use std::collections::BTreeMap;
use std::fmt;

use bytes::Bytes;
use http::{HeaderMap, HeaderValue};
use time::OffsetDateTime;
use whl_core::entropy::random_bytes;
use whl_secrets::{Secret, SecretName};
use whl_template::{
    ChaChaRandom, Context, EndpointData, RandomSeed, RenderEnv, RenderLimits, RenderTarget,
    Secrets, SystemClock, Template,
};

use crate::request::{CompiledRequest, MAX_BODY_BYTES, MAX_URL_BYTES};

const SEED_BYTES: usize = 32;

pub struct RenderedRequest {
    pub url: String,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl fmt::Debug for RenderedRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RenderedRequest")
            .field("url_bytes", &self.url.len())
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RenderScene<'a> {
    pub now: OffsetDateTime,
    pub endpoint_id: &'a str,
    pub public_url: &'a str,
}

pub fn render_request(
    compiled: &CompiledRequest,
    scene: RenderScene<'_>,
    secrets: &[(SecretName, Secret)],
) -> Result<RenderedRequest, String> {
    let context = context_for(scene, secrets);
    let seed = random_bytes::<SEED_BYTES>()
        .map_err(|_| "the random source failed while rendering".to_owned())?;
    let mut random = ChaChaRandom::from_seed(&RandomSeed::from_bytes(&seed));
    let clock = SystemClock::new();
    let mut env = RenderEnv::new(&mut random, &clock);

    let mut limits = RenderLimits::for_target(RenderTarget::Body);
    limits.max_output_bytes = MAX_URL_BYTES;
    let url = render_one(&compiled.url, &context, &limits, &mut env)
        .map_err(|error| format!("request.url: {error}"))?;

    let header_limits = RenderLimits::for_target(RenderTarget::HeaderValue);
    let mut headers = HeaderMap::new();
    for (index, header) in compiled.headers.iter().enumerate() {
        let text = render_one(&header.value, &context, &header_limits, &mut env)
            .map_err(|error| format!("request.headers.{index}.value: {error}"))?;
        let mut value = HeaderValue::from_str(&text)
            .map_err(|_| format!("request.headers.{index}.value: is not a valid header value"))?;
        value.set_sensitive(true);
        headers.append(header.name.clone(), value);
    }

    let body = match &compiled.body {
        Some(template) => {
            let mut body_limits = RenderLimits::for_target(RenderTarget::Body);
            body_limits.max_output_bytes = MAX_BODY_BYTES;
            let text = render_one(template, &context, &body_limits, &mut env)
                .map_err(|error| format!("request.body: {error}"))?;
            Bytes::from(text)
        }
        None => Bytes::new(),
    };
    Ok(RenderedRequest { url, headers, body })
}

fn context_for(scene: RenderScene<'_>, secrets: &[(SecretName, Secret)]) -> Context {
    let mut context = Context::new(scene.now);
    context.endpoint = EndpointData {
        id: scene.endpoint_id.to_owned(),
        url: format!("{}/{}", scene.public_url, scene.endpoint_id),
        title: String::new(),
    };
    let values: BTreeMap<String, String> = secrets
        .iter()
        .map(|(name, secret)| (name.unpack().to_owned(), secret.unpack().to_owned()))
        .collect();
    context.secrets = Secrets::new(values);
    context
}

fn render_one(
    template: &Template,
    context: &Context,
    limits: &RenderLimits,
    env: &mut RenderEnv<'_>,
) -> Result<String, String> {
    let outcome = template.render(context, limits, env);
    if let Some(error) = outcome.errors().first() {
        return Err(error.to_string());
    }
    if let Some(missing) = outcome.missing().first() {
        return Err(format!("{} is not defined", missing.reference));
    }
    Ok(outcome.output().to_owned())
}
