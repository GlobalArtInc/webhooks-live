use serde_json::json;
use whl_script::ScriptCompiler;
use whl_template::{
    ChaChaRandom, Context, Policy, RandomSeed, RenderEnv, RenderLimits, RenderOutcome,
    RenderTarget, SystemClock, Template,
};

use crate::after::DeferredStep;
use crate::failure::{Failure, FailureCode};
use crate::machine::{ForwardCall, NotifyCall, OutboundCall, StoreS3Call};
use crate::model::{Action, BodySource, Step};
use crate::set::load_set;

pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;

pub fn deferred_step(deferred: &DeferredStep, scripts: &ScriptCompiler) -> Result<Step, Failure> {
    let rule = json!({
        "name": deferred.rule_name,
        "enabled": true,
        "condition": {},
        "steps": [deferred.action],
    });
    let mut rules = load_set(&[rule], scripts)
        .map_err(|_| Failure::new(FailureCode::Internal, "a deferred step no longer validates"))?;
    rules
        .pop()
        .and_then(|rule| rule.steps.into_iter().next())
        .ok_or_else(|| Failure::new(FailureCode::Internal, "a deferred step is missing"))
}

pub fn side_effect_call(
    action: &Action,
    message: Option<String>,
    payload: Option<&str>,
) -> Result<OutboundCall, Failure> {
    match action {
        Action::Forward {
            forward_rule_id,
            target_url,
            method,
            content_type,
            header_allowlist,
            body_from,
        } => {
            if *body_from == BodySource::Payload && payload.is_none() {
                return Err(Failure::new(
                    FailureCode::Template,
                    "body_from is payload but no transform step wrote the payload",
                ));
            }
            Ok(OutboundCall::Forward(ForwardCall {
                forward_rule_id: *forward_rule_id,
                target_url: target_url.clone(),
                method: method.clone(),
                content_type: content_type.clone(),
                header_allowlist: header_allowlist.clone(),
                body_from: *body_from,
                payload: payload.map(str::to_owned),
            }))
        }
        Action::Notify { channel_id, .. } => Ok(OutboundCall::Notify(NotifyCall {
            channel_id: *channel_id,
            message,
        })),
        Action::StoreS3 {
            bucket_url,
            prefix,
            region,
            access_key_secret,
            secret_key_secret,
            content,
        } => Ok(OutboundCall::StoreS3(StoreS3Call {
            bucket_url: bucket_url.clone(),
            prefix: prefix.clone(),
            region: region.clone(),
            access_key_secret: access_key_secret.clone(),
            secret_key_secret: secret_key_secret.clone(),
            content: *content,
        })),
        _ => Err(Failure::new(
            FailureCode::Internal,
            "the step is not a side effect",
        )),
    }
}

pub fn render_message(
    source: &str,
    context: &Context,
    seed: &RandomSeed,
) -> Result<String, Failure> {
    let template =
        Template::parse(source, &Policy::response(RenderTarget::Body)).map_err(|_| {
            Failure::new(
                FailureCode::Template,
                "the message template no longer parses",
            )
        })?;
    let mut limits = RenderLimits::for_target(RenderTarget::Body);
    limits.max_output_bytes = MAX_MESSAGE_BYTES;
    let mut random = ChaChaRandom::from_seed(seed);
    let clock = SystemClock::new();
    let mut env = RenderEnv::new(&mut random, &clock);
    match template.render(context, &limits, &mut env) {
        RenderOutcome::Clean(rendered) => Ok(rendered.output),
        RenderOutcome::Partial(_) => Err(Failure::new(
            FailureCode::Template,
            "the message template did not render",
        )),
    }
}
