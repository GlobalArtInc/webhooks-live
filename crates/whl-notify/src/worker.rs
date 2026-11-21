use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use metrics::{counter, gauge};
use serde_json::Value;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use whl_core::endpoint_id::EndpointId;
use whl_forward::RetryPolicy;
use whl_secrets::{Keyring, Secret, SecretName};
use whl_store::Store;
use whl_store::forward::{RecordOutcome, Settlement};
use whl_store::notify::{
    ChannelKind, ClaimPolicy, ClaimedDelivery, DeliveryKind, DeliveryResult, MailRecipient,
    QuotaCap, QuotaVerdict,
};
use whl_telemetry::series::{NOTIFICATION_QUEUE_DEPTH, NOTIFICATIONS_TOTAL};

use crate::adapter::{AdapterInput, Destination, MailContext, adapter_for};
use crate::address::address_digest;
use crate::disable::issue_disable_token;
use crate::notice::{ComposeInput, compose};
use crate::secret::channel_secret_name;
use crate::settings::{MAX_CLAIM_BATCH, NotifySettings, SUMMARY_LOOK_BACK_HOURS, SettingsError};
use crate::target::targets_instance;
use crate::transport::{Reply, SendError, Transport};

const QUEUE_DEPTH_INTERVAL: Duration = Duration::from_secs(10);
const RECORD_TRIES: u32 = 3;
const RECORD_RETRY_DELAY: Duration = Duration::from_millis(500);
const WAKE_RECONNECT_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(300);
const GLOBAL_SUBJECT: &str = "g:ownerless";
const REDACTED: &str = "[secret]";
const MIN_REDACTED_BYTES: usize = 4;

#[derive(Debug)]
pub struct NotifyHandle {
    task: JoinHandle<()>,
}

impl NotifyHandle {
    pub async fn join(&mut self) {
        if let Err(error) = (&mut self.task).await {
            tracing::error!(%error, "the notification worker ended abnormally");
        }
    }

    pub fn abort(&self) {
        self.task.abort();
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

pub struct NotifyWorker {
    store: Store,
    keyring: Arc<Keyring>,
    transport: Transport,
    settings: NotifySettings,
}

struct Shared {
    store: Store,
    keyring: Arc<Keyring>,
    transport: Transport,
    settings: NotifySettings,
    wake: Notify,
    slots: Arc<Semaphore>,
}

impl NotifyWorker {
    pub fn new(
        store: Store,
        keyring: Arc<Keyring>,
        transport: Transport,
        settings: NotifySettings,
    ) -> Result<Self, SettingsError> {
        settings.validate()?;
        Ok(Self {
            store,
            keyring,
            transport,
            settings,
        })
    }

    pub fn spawn(self, shutdown: impl Future<Output = ()> + Send + 'static) -> NotifyHandle {
        NotifyHandle {
            task: tokio::spawn(self.run(shutdown)),
        }
    }

    pub async fn run(self, shutdown: impl Future<Output = ()>) {
        let shared = Arc::new(Shared {
            slots: Arc::new(Semaphore::new(self.settings.concurrency)),
            store: self.store,
            keyring: self.keyring,
            transport: self.transport,
            settings: self.settings,
            wake: Notify::new(),
        });
        let listener = tokio::spawn(listen_for_wakeups(Arc::clone(&shared)));
        let mut attempts = JoinSet::new();
        tracing::info!(
            concurrency = shared.settings.concurrency,
            max_attempts = shared.settings.max_attempts,
            "the notification worker started"
        );
        tokio::pin!(shutdown);
        claim_loop(&shared, &mut attempts, &mut shutdown).await;
        listener.abort();
        while let Some(joined) = attempts.join_next().await {
            report_join(joined);
        }
        tracing::info!("the notification worker finished");
    }
}

async fn claim_loop(
    shared: &Arc<Shared>,
    attempts: &mut JoinSet<()>,
    shutdown: &mut (impl Future<Output = ()> + Unpin),
) {
    let mut last_depth: Option<Instant> = None;
    let mut last_housekeeping: Option<Instant> = None;
    loop {
        while let Some(joined) = attempts.try_join_next() {
            report_join(joined);
        }
        let first = tokio::select! {
            biased;
            () = &mut *shutdown => return,
            permit = Arc::clone(&shared.slots).acquire_owned() => permit,
        };
        let Ok(first) = first else {
            return;
        };
        let mut permits = vec![first];
        while permits.len() < MAX_CLAIM_BATCH {
            match Arc::clone(&shared.slots).try_acquire_owned() {
                Ok(permit) => permits.push(permit),
                Err(_) => break,
            }
        }
        if due(
            &mut last_housekeeping,
            shared.settings.housekeeping_interval,
        ) {
            housekeeping(shared).await;
        }
        if due(&mut last_depth, QUEUE_DEPTH_INTERVAL) {
            publish_depth(shared).await;
        }
        let requested = permits.len();
        let claimed = claim(shared, requested).await;
        let drained = claimed.as_ref().is_ok_and(|batch| batch.len() == requested);
        match claimed {
            Ok(batch) => {
                for (delivery, permit) in batch.into_iter().zip(permits.drain(..)) {
                    attempts.spawn(run_attempt(Arc::clone(shared), delivery, permit));
                }
            }
            Err(error) => tracing::warn!(%error, "cannot claim notification deliveries"),
        }
        drop(permits);
        if drained {
            continue;
        }
        let stopped = tokio::select! {
            biased;
            () = &mut *shutdown => true,
            () = shared.wake.notified() => false,
            () = tokio::time::sleep(shared.settings.poll_interval) => false,
        };
        if stopped {
            return;
        }
    }
}

fn due(last: &mut Option<Instant>, every: Duration) -> bool {
    let now = Instant::now();
    match last {
        Some(previous) if now.duration_since(*previous) < every => false,
        _ => {
            *last = Some(now);
            true
        }
    }
}

fn max_attempts(shared: &Shared) -> i32 {
    i32::try_from(shared.settings.max_attempts).unwrap_or(i32::MAX)
}

async fn claim(
    shared: &Shared,
    requested: usize,
) -> Result<Vec<ClaimedDelivery>, whl_store::notify::NotifyStoreError> {
    shared
        .store
        .claim_deliveries(ClaimPolicy {
            limit: i64::try_from(requested).unwrap_or(i64::MAX),
            max_attempts: max_attempts(shared),
            lease: shared.settings.lease,
        })
        .await
}

async fn housekeeping(shared: &Shared) {
    match shared
        .store
        .fail_exhausted_deliveries(max_attempts(shared))
        .await
    {
        Ok(0) => {}
        Ok(count) => tracing::warn!(count, "failed notifications whose last lease expired"),
        Err(error) => tracing::warn!(%error, "cannot fail exhausted notifications"),
    }
    match shared
        .store
        .enqueue_summaries(SUMMARY_LOOK_BACK_HOURS)
        .await
    {
        Ok(0) => {}
        Ok(count) => tracing::info!(count, "queued suppression summaries"),
        Err(error) => tracing::warn!(%error, "cannot queue suppression summaries"),
    }
    if let Err(error) = shared.store.prune_notification_counters().await {
        tracing::warn!(%error, "cannot prune notification counters");
    }
    if let Err(error) = shared.store.prune_old_deliveries().await {
        tracing::warn!(%error, "cannot prune old notification deliveries");
    }
}

async fn publish_depth(shared: &Shared) {
    match shared.store.notification_queue_depth().await {
        Ok(depth) => gauge!(NOTIFICATION_QUEUE_DEPTH).set(depth as f64),
        Err(error) => tracing::warn!(%error, "cannot read the notification queue depth"),
    }
}

async fn listen_for_wakeups(shared: Arc<Shared>) {
    loop {
        match shared.store.listen_for_notify_wakeups().await {
            Ok(mut listener) => {
                shared.wake.notify_one();
                loop {
                    match listener.wait().await {
                        Ok(()) => shared.wake.notify_one(),
                        Err(error) => {
                            tracing::warn!(%error, "the notification wake listener lost its connection");
                            break;
                        }
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "cannot listen for notification wakeups"),
        }
        tokio::time::sleep(WAKE_RECONNECT_DELAY).await;
    }
}

fn report_join(joined: Result<(), tokio::task::JoinError>) {
    if let Err(error) = joined {
        tracing::error!(%error, "a notification attempt task ended abnormally");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Sent,
    Failed,
    Retrying,
}

struct Finished {
    status: Option<i16>,
    error: Option<String>,
    settlement: Settlement,
    outcome: Outcome,
}

impl Finished {
    fn failed(error: impl Into<String>) -> Self {
        Self {
            status: None,
            error: Some(error.into()),
            settlement: Settlement::Failed,
            outcome: Outcome::Failed,
        }
    }
}

struct Opened {
    url: Option<Secret>,
    token: Option<Secret>,
}

async fn run_attempt(shared: Arc<Shared>, delivery: ClaimedDelivery, permit: OwnedSemaphorePermit) {
    let _slot = permit;
    let Some(finished) = process(&shared, &delivery).await else {
        return;
    };
    match finished.outcome {
        Outcome::Sent => count(delivery.channel.kind, "sent"),
        Outcome::Failed => count(delivery.channel.kind, "failed"),
        Outcome::Retrying => {}
    }
    let result = DeliveryResult {
        id: delivery.id,
        attempt: delivery.attempt,
        status: finished.status,
        error: finished.error,
    };
    store_result(&shared, &result, finished.settlement).await;
}

fn count(kind: ChannelKind, outcome: &'static str) {
    counter!(NOTIFICATIONS_TOTAL, "channel" => kind.as_str(), "outcome" => outcome).increment(1);
}

fn caps(
    delivery: &ClaimedDelivery,
    shared: &Shared,
    recipient: Option<&MailRecipient>,
) -> Vec<QuotaCap> {
    let limits = shared.settings.limits;
    let channel = &delivery.channel;
    let mut caps = vec![
        QuotaCap::hourly(
            format!("c:{}", delivery.channel_id.hyphenated()),
            channel.per_hour,
        ),
        QuotaCap::hourly(
            format!("e:{}", channel.endpoint_id),
            limits.endpoint_per_hour,
        ),
    ];
    match (&channel.owner_id, &channel.creator_net) {
        (Some(owner), _) => caps.push(QuotaCap::hourly(
            format!("u:{}", owner.hyphenated()),
            limits.owner_per_hour,
        )),
        (None, Some(network)) => caps.push(QuotaCap::hourly(
            format!("n:{network}"),
            limits.network_per_hour,
        )),
        (None, None) => caps.push(QuotaCap::hourly(
            GLOBAL_SUBJECT.to_owned(),
            limits.global_per_hour,
        )),
    }
    if let Some(recipient) = recipient {
        let email = shared.settings.email;
        caps.push(QuotaCap::daily(
            format!("m:{}", recipient.user_id.hyphenated()),
            email.per_user_per_day,
        ));
        caps.push(QuotaCap::daily(
            format!("ma:{}", address_digest(&recipient.address)),
            email.per_address_per_day,
        ));
    }
    caps
}

async fn resolve_recipient(
    shared: &Shared,
    delivery: &ClaimedDelivery,
) -> Result<MailRecipient, Option<Finished>> {
    if delivery.channel.owner_id.is_none() {
        return Err(Some(Finished::failed(
            "email_requires_owner: email notifications need an endpoint with an owner",
        )));
    }
    match shared.store.verified_recipient(delivery.channel_id).await {
        Ok(Some(recipient)) => Ok(recipient),
        Ok(None) => Err(Some(Finished::failed(
            "recipient_unverified: the channel has no verified address",
        ))),
        Err(error) => {
            tracing::warn!(delivery_id = %delivery.id, %error, "cannot resolve the recipient of an email delivery");
            Err(None)
        }
    }
}

async fn process(shared: &Shared, delivery: &ClaimedDelivery) -> Option<Finished> {
    let Some(adapter) = adapter_for(delivery.channel.kind) else {
        return Some(Finished::failed(
            "unsupported_kind: this channel kind is not available",
        ));
    };
    if !delivery.channel.enabled {
        return Some(Finished::failed(
            "channel_disabled: the channel is disabled",
        ));
    }
    let recipient = if delivery.channel.kind == ChannelKind::Email {
        match resolve_recipient(shared, delivery).await {
            Ok(recipient) => Some(recipient),
            Err(finished) => return finished,
        }
    } else {
        None
    };
    if !delivery.quota_checked {
        let charged = shared
            .store
            .charge_delivery(
                delivery.id,
                delivery.attempt,
                &caps(delivery, shared, recipient.as_ref()),
            )
            .await;
        match charged {
            Ok(QuotaVerdict::Allowed) => {}
            Ok(QuotaVerdict::Suppressed) => {
                count(delivery.channel.kind, "suppressed");
                return None;
            }
            Ok(QuotaVerdict::Superseded) => return None,
            Err(error) => {
                tracing::warn!(delivery_id = %delivery.id, %error, "cannot charge a notification quota");
                return None;
            }
        }
    }
    let capture = match (delivery.kind, delivery.capture_id) {
        (DeliveryKind::Single, Some(capture_id)) => {
            match shared.store.load_notify_capture(capture_id).await {
                Ok(Some(capture)) => Some(capture),
                Ok(None) => return Some(Finished::failed("capture_gone: the capture was removed")),
                Err(error) => {
                    tracing::warn!(delivery_id = %delivery.id, %error, "cannot load the capture of a delivery");
                    return None;
                }
            }
        }
        (DeliveryKind::Single, None) => {
            return Some(Finished::failed(
                "capture_gone: the delivery has no capture",
            ));
        }
        _ => None,
    };
    let opened = match open_secrets(shared, delivery).await {
        Ok(opened) => opened,
        Err(finished) => return finished,
    };
    let notice = compose(&ComposeInput {
        delivery,
        capture: capture.as_ref(),
        public_url: &shared.settings.public_url,
    });
    let disable_token = match &recipient {
        Some(_) => match issue_disable_token(&shared.keyring, delivery.channel_id) {
            Ok(token) => Some(token),
            Err(error) => {
                tracing::error!(delivery_id = %delivery.id, %error, "cannot sign a disable token");
                return Some(Finished::failed(
                    "internal: the disable link cannot be signed",
                ));
            }
        },
        None => None,
    };
    let mail = recipient
        .as_ref()
        .zip(disable_token.as_deref())
        .map(|(recipient, token)| MailContext {
            recipient: &recipient.address,
            disable_token: token,
            public_url: &shared.settings.public_url,
        });
    let prepared = adapter.prepare(&AdapterInput {
        config: &delivery.channel.config,
        url: opened.url.as_ref().map(Secret::unpack),
        token: opened.token.as_ref().map(Secret::unpack),
        mail,
        notice: &notice,
        delivery: delivery.id,
    });
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return Some(Finished::failed(format!("invalid_channel: {error}"))),
    };
    if let Destination::Guarded(url) = &prepared.destination
        && targets_instance(url, &shared.settings.public_url)
    {
        return Some(Finished::failed(
            "refused_target: the URL points at this instance",
        ));
    }
    let sent = shared.transport.send(&prepared).await;
    Some(conclude(shared, delivery, &opened, sent))
}

async fn open_secrets(
    shared: &Shared,
    delivery: &ClaimedDelivery,
) -> Result<Opened, Option<Finished>> {
    let channel = &delivery.channel;
    let unavailable = || {
        Some(Finished::failed(
            "secret_unavailable: the channel secret cannot be opened",
        ))
    };
    let endpoint = EndpointId::parse(&channel.endpoint_id).map_err(|_| unavailable())?;
    let url = match &channel.sealed {
        Some(sealed) => {
            let name = channel_secret_name(delivery.channel_id).map_err(|_| unavailable())?;
            Some(
                shared
                    .keyring
                    .open(&endpoint, &name, sealed)
                    .map_err(|error| {
                        tracing::warn!(delivery_id = %delivery.id, %error, "cannot open a channel secret");
                        unavailable()
                    })?,
            )
        }
        None => None,
    };
    let token = if channel.kind == ChannelKind::Telegram {
        Some(open_token(shared, delivery, &endpoint, &channel.config).await?)
    } else {
        None
    };
    Ok(Opened { url, token })
}

async fn open_token(
    shared: &Shared,
    delivery: &ClaimedDelivery,
    endpoint: &EndpointId,
    config: &Value,
) -> Result<Secret, Option<Finished>> {
    let name = config
        .get("token_secret")
        .and_then(Value::as_str)
        .and_then(|text| SecretName::parse(text).ok())
        .ok_or_else(|| {
            Some(Finished::failed(
                "invalid_channel: the token secret is not named",
            ))
        })?;
    let sealed = match shared.store.get_sealed_secret(endpoint, &name).await {
        Ok(Some(sealed)) => sealed,
        Ok(None) => {
            return Err(Some(Finished::failed(format!(
                "secret_missing: the secret {name} does not exist"
            ))));
        }
        Err(error) => {
            tracing::warn!(delivery_id = %delivery.id, %error, "cannot read the token secret");
            return Err(None);
        }
    };
    shared
        .keyring
        .open(endpoint, &name, &sealed)
        .map_err(|error| {
            tracing::warn!(delivery_id = %delivery.id, %error, "cannot open the token secret");
            Some(Finished::failed(
                "secret_unavailable: the token secret cannot be opened",
            ))
        })
}

fn conclude(
    shared: &Shared,
    delivery: &ClaimedDelivery,
    opened: &Opened,
    sent: Result<Reply, SendError>,
) -> Finished {
    let policy = RetryPolicy::new(shared.settings.max_attempts);
    let attempt = u32::try_from(delivery.attempt).unwrap_or(u32::MAX);
    let retry = |delay_hint: Option<Duration>| {
        if policy.has_attempts_left(attempt) {
            let delay = delay_hint.map_or_else(
                || policy.delay_after(attempt),
                |hint| hint.min(MAX_RETRY_AFTER),
            );
            Settlement::RetryIn(delay)
        } else {
            Settlement::Failed
        }
    };
    let finished = |status, error: Option<String>, settlement| Finished {
        status,
        error: error.map(|text| redact(&text, opened)),
        outcome: match settlement {
            Settlement::Succeeded => Outcome::Sent,
            Settlement::Failed => Outcome::Failed,
            Settlement::RetryIn(_) => Outcome::Retrying,
        },
        settlement,
    };
    match sent {
        Ok(reply) => {
            let status = i16::try_from(reply.status).ok();
            match reply.status {
                200..=299 => finished(status, None, Settlement::Succeeded),
                429 => finished(
                    status,
                    Some("http_429: the vendor rate limited the request".to_owned()),
                    retry(retry_after(&reply)),
                ),
                408 | 500..=599 => finished(
                    status,
                    Some(format!(
                        "http_{}: the target answered with an error",
                        reply.status
                    )),
                    retry(None),
                ),
                other => finished(
                    status,
                    Some(format!("http_{other}: the target refused the notification")),
                    Settlement::Failed,
                ),
            }
        }
        Err(error) if error.is_retryable() => finished(None, Some(error.to_string()), retry(None)),
        Err(error) => finished(None, Some(error.to_string()), Settlement::Failed),
    }
}

fn retry_after(reply: &Reply) -> Option<Duration> {
    let body: Value = serde_json::from_slice(&reply.body).ok()?;
    let seconds = body.get("parameters")?.get("retry_after")?.as_u64()?;
    Some(Duration::from_secs(seconds))
}

fn redact(text: &str, opened: &Opened) -> String {
    let mut redacted = text.to_owned();
    for secret in [&opened.url, &opened.token].into_iter().flatten() {
        if secret.unpack().len() >= MIN_REDACTED_BYTES {
            redacted = redacted.replace(secret.unpack(), REDACTED);
        }
    }
    redacted
}

async fn store_result(shared: &Shared, result: &DeliveryResult, settlement: Settlement) {
    for tries in 1..=RECORD_TRIES {
        match shared.store.settle_delivery(result, settlement).await {
            Ok(RecordOutcome::Recorded) => return,
            Ok(RecordOutcome::Superseded) => {
                tracing::warn!(
                    delivery_id = %result.id,
                    attempt = result.attempt,
                    "the delivery was reclaimed or deleted before its result was recorded"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(delivery_id = %result.id, tries, %error, "cannot record a notification result");
                tokio::time::sleep(RECORD_RETRY_DELAY * tries).await;
            }
        }
    }
    tracing::error!(
        delivery_id = %result.id,
        attempt = result.attempt,
        "the notification result was lost; the delivery is retried after its lease expires"
    );
}
