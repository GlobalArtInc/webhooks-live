use futures_util::future::join_all;
use serde_json::Value;
use whl_core::endpoint_id::EndpointId;
use whl_store::Store;
use whl_store::Uuid;
use whl_store::notify::{ClaimedDelivery, PushResults, PushSubscription};

use crate::adapter::{Adapter, AdapterInput};
use crate::notice::Notice;
use crate::push::{PushService, PushTarget};
use crate::transport::{Reply, SendError, Transport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PushOutcome {
    Delivered { status: u16 },
    Retry { status: Option<u16>, detail: String },
    Failed { status: Option<u16>, detail: String },
    Deferred,
}

impl PushOutcome {
    fn failed(detail: &str) -> Self {
        Self::Failed {
            status: None,
            detail: detail.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Accepted,
    Gone,
    Retry,
    Refused,
}

struct Sent {
    id: Uuid,
    verdict: Verdict,
    status: Option<u16>,
    detail: String,
}

pub(crate) struct PushRoute<'a> {
    pub store: &'a Store,
    pub transport: &'a Transport,
    pub service: &'a PushService,
    pub adapter: &'a dyn Adapter,
}

pub(crate) async fn deliver(
    route: &PushRoute<'_>,
    delivery: &ClaimedDelivery,
    notice: &Notice,
) -> PushOutcome {
    let Some(creator) = delivery.channel.created_by else {
        return PushOutcome::failed("invalid_channel: the channel has no creator to notify");
    };
    let Ok(endpoint) = EndpointId::parse(&delivery.channel.endpoint_id) else {
        return PushOutcome::failed("invalid_channel: the endpoint id is not valid");
    };
    match route.store.user_may_view_endpoint(&endpoint, creator).await {
        Ok(true) => {}
        Ok(false) => {
            return PushOutcome::failed(
                "creator_lost_access: the channel creator can no longer view the endpoint",
            );
        }
        Err(error) => {
            tracing::warn!(delivery_id = %delivery.id, %error, "cannot check the push recipient");
            return PushOutcome::Deferred;
        }
    }
    let subscriptions = match route.store.push_subscriptions_of(creator).await {
        Ok(subscriptions) => subscriptions,
        Err(error) => {
            tracing::warn!(delivery_id = %delivery.id, %error, "cannot load push subscriptions");
            return PushOutcome::Deferred;
        }
    };
    if subscriptions.is_empty() {
        return PushOutcome::failed(
            "no_subscriptions: the channel creator has no push subscriptions",
        );
    }
    let sends = join_all(
        subscriptions
            .iter()
            .map(|subscription| send_one(route, subscription, notice, delivery.id)),
    )
    .await;
    record(route.store, &sends).await;
    aggregate(&sends)
}

async fn send_one(
    route: &PushRoute<'_>,
    subscription: &PushSubscription,
    notice: &Notice,
    delivery: Uuid,
) -> Sent {
    let target = PushTarget {
        endpoint: &subscription.endpoint_url,
        p256dh: &subscription.p256dh,
        auth: &subscription.auth,
        vapid: route.service.vapid(),
    };
    let config = Value::Null;
    let prepared = route.adapter.prepare(&AdapterInput {
        config: &config,
        url: None,
        token: None,
        notice,
        delivery,
        push: Some(&target),
    });
    match prepared {
        Ok(prepared) => settle(subscription.id, route.transport.send(&prepared).await),
        Err(error) => Sent {
            id: subscription.id,
            verdict: Verdict::Refused,
            status: None,
            detail: format!("invalid_subscription: {error}"),
        },
    }
}

fn settle(id: Uuid, sent: Result<Reply, SendError>) -> Sent {
    match sent {
        Ok(reply) => {
            let status = reply.status;
            let (verdict, detail) = match status {
                200..=299 => (Verdict::Accepted, String::new()),
                404 | 410 => (
                    Verdict::Gone,
                    format!("http_{status}: the push service no longer knows the subscription"),
                ),
                408 | 429 | 500..=599 => (
                    Verdict::Retry,
                    format!("http_{status}: the push service answered with an error"),
                ),
                _ => (
                    Verdict::Refused,
                    format!("http_{status}: the push service refused the notification"),
                ),
            };
            Sent {
                id,
                verdict,
                status: Some(status),
                detail,
            }
        }
        Err(error) if error.is_retryable() => Sent {
            id,
            verdict: Verdict::Retry,
            status: None,
            detail: error.to_string(),
        },
        Err(error) => Sent {
            id,
            verdict: Verdict::Refused,
            status: None,
            detail: error.to_string(),
        },
    }
}

fn ids_where(sends: &[Sent], wanted: &[Verdict]) -> Vec<Uuid> {
    sends
        .iter()
        .filter(|sent| wanted.contains(&sent.verdict))
        .map(|sent| sent.id)
        .collect()
}

fn rejected_ids(sends: &[Sent]) -> Vec<Uuid> {
    sends
        .iter()
        .filter(|sent| {
            sent.verdict == Verdict::Refused && matches!(sent.status, Some(400 | 401 | 403))
        })
        .map(|sent| sent.id)
        .collect()
}

async fn record(store: &Store, sends: &[Sent]) {
    let gone = ids_where(sends, &[Verdict::Gone]);
    let delivered = ids_where(sends, &[Verdict::Accepted]);
    let rejected = rejected_ids(sends);
    match store.remove_push_subscriptions(&gone).await {
        Ok(0) => {}
        Ok(count) => {
            tracing::info!(
                count,
                "removed push subscriptions that their push service no longer knows"
            );
        }
        Err(error) => tracing::warn!(%error, "cannot remove expired push subscriptions"),
    }
    let results = PushResults {
        delivered: &delivered,
        rejected: &rejected,
    };
    match store.record_push_results(results).await {
        Ok(0) => {}
        Ok(count) => {
            tracing::info!(
                count,
                "removed push subscriptions that their push service kept rejecting"
            );
        }
        Err(error) => tracing::warn!(%error, "cannot record push delivery results"),
    }
}

fn aggregate(sends: &[Sent]) -> PushOutcome {
    if let Some(sent) = sends.iter().find(|sent| sent.verdict == Verdict::Accepted) {
        return PushOutcome::Delivered {
            status: sent.status.unwrap_or(201),
        };
    }
    if let Some(sent) = sends.iter().find(|sent| sent.verdict == Verdict::Retry) {
        return PushOutcome::Retry {
            status: sent.status,
            detail: sent.detail.clone(),
        };
    }
    if sends.iter().all(|sent| sent.verdict == Verdict::Gone) {
        return PushOutcome::Failed {
            status: sends.first().and_then(|sent| sent.status),
            detail: "subscription_gone: every subscription was removed by its push service"
                .to_owned(),
        };
    }
    let refused = sends
        .iter()
        .find(|sent| sent.verdict == Verdict::Refused)
        .or(sends.first());
    PushOutcome::Failed {
        status: refused.and_then(|sent| sent.status),
        detail: refused.map_or_else(
            || "http_0: the push service refused the notification".to_owned(),
            |sent| sent.detail.clone(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(verdict: Verdict, status: Option<u16>) -> Sent {
        Sent {
            id: Uuid::now_v7(),
            verdict,
            status,
            detail: format!("detail {verdict:?}"),
        }
    }

    #[test]
    fn one_accepted_subscription_delivers_whatever_the_others_did() {
        let outcome = aggregate(&[
            sent(Verdict::Retry, Some(503)),
            sent(Verdict::Gone, Some(410)),
            sent(Verdict::Accepted, Some(201)),
        ]);
        assert_eq!(outcome, PushOutcome::Delivered { status: 201 });
    }

    #[test]
    fn a_retryable_failure_without_any_acceptance_is_retried() {
        let outcome = aggregate(&[
            sent(Verdict::Gone, Some(410)),
            sent(Verdict::Retry, Some(503)),
        ]);
        assert!(matches!(
            outcome,
            PushOutcome::Retry {
                status: Some(503),
                ..
            }
        ));
    }

    #[test]
    fn only_gone_subscriptions_end_the_delivery_as_gone_and_refusals_end_it_as_failed() {
        let gone = aggregate(&[
            sent(Verdict::Gone, Some(410)),
            sent(Verdict::Gone, Some(404)),
        ]);
        assert!(matches!(
            &gone,
            PushOutcome::Failed { status: Some(410), detail } if detail.starts_with("subscription_gone")
        ));
        let refused = aggregate(&[
            sent(Verdict::Gone, Some(410)),
            sent(Verdict::Refused, Some(403)),
        ]);
        assert!(matches!(
            refused,
            PushOutcome::Failed {
                status: Some(403),
                ..
            }
        ));
    }

    #[test]
    fn only_400_401_and_403_refusals_count_toward_removal() {
        let counted = [
            sent(Verdict::Refused, Some(400)),
            sent(Verdict::Refused, Some(401)),
            sent(Verdict::Refused, Some(403)),
        ];
        let ignored = [
            sent(Verdict::Refused, Some(413)),
            sent(Verdict::Refused, None),
            sent(Verdict::Retry, Some(503)),
            sent(Verdict::Gone, Some(410)),
            sent(Verdict::Accepted, Some(201)),
        ];
        let all: Vec<Sent> = counted.into_iter().chain(ignored).collect();
        let expected: Vec<Uuid> = all[..3].iter().map(|sent| sent.id).collect();
        assert_eq!(rejected_ids(&all), expected);
    }

    #[test]
    fn replies_are_classified_by_status() {
        let id = Uuid::now_v7();
        let reply = |status| {
            Ok(Reply {
                status,
                body: bytes::Bytes::new(),
            })
        };
        for (status, expected) in [
            (201, Verdict::Accepted),
            (200, Verdict::Accepted),
            (404, Verdict::Gone),
            (410, Verdict::Gone),
            (429, Verdict::Retry),
            (503, Verdict::Retry),
            (400, Verdict::Refused),
            (401, Verdict::Refused),
            (403, Verdict::Refused),
            (413, Verdict::Refused),
        ] {
            assert_eq!(settle(id, reply(status)).verdict, expected, "{status}");
        }
        assert_eq!(
            settle(id, Err(SendError::Transient("timeout".to_owned()))).verdict,
            Verdict::Retry
        );
        assert_eq!(
            settle(id, Err(SendError::Refused("target".to_owned()))).verdict,
            Verdict::Refused
        );
    }
}
