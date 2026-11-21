use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use whl_capture::{RuleFacts, load_rule_variables};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_rules::{
    Action, CompiledRules, DeferredStep, Entry, Failure, FailureCode, Snapshot, Step, StepClass,
    deferred_from_plan, deferred_step, entries_from_value, load_stored, plan_payload,
    plan_resume_step, rules_from_plan, snapshots_from_plan, steps_from_value,
};
use whl_secrets::{MIN_REDACTED_VALUE_BYTES, Secret, SecretName};
use whl_store::CaptureDetail;
use whl_store::rule_exec::ClaimedRun;
use whl_store::rule_runs::{MAX_HOPS, detail_hop};
use whl_template::{Context, Secrets, StepData};

use crate::scrub::{Opened, Scrubber};
use crate::worker::Shared;

const VARIABLES_ACQUIRE: Duration = Duration::from_secs(1);
const VARIABLES_DEADLINE: Duration = Duration::from_millis(500);

pub(crate) enum Prep {
    Fail(Failure),
    Halt,
}

fn internal(detail: &str) -> Prep {
    Prep::Fail(Failure::new(FailureCode::Internal, detail))
}

pub(crate) struct Deferred {
    pub item: DeferredStep,
    pub snapshot: Snapshot,
    pub step: Step,
}

pub(crate) struct Prepared {
    pub endpoint: EndpointId,
    pub detail: CaptureDetail,
    pub hop: u32,
    pub facts: RuleFacts,
    pub compiled: Arc<CompiledRules>,
    pub deferred: Vec<Deferred>,
    pub machine_start: usize,
    pub payload: Option<String>,
    pub opened: Opened,
    pub scrub: Scrubber,
    pub context: Context,
    pub log: Vec<Entry>,
    pub steps: BTreeMap<String, StepData>,
}

struct Compiled {
    rules: Arc<CompiledRules>,
    deferred: Vec<Deferred>,
}

fn compile(plan: Value, compiler: whl_script::ScriptCompiler) -> Result<Compiled, Prep> {
    let stored = rules_from_plan(&plan).map_err(|error| {
        tracing::error!(%error, "the stored rule plan has the wrong shape");
        internal("the stored rules are unreadable")
    })?;
    let sources = load_stored(&stored, &compiler).map_err(|_| {
        tracing::error!("the stored rules no longer validate");
        internal("the stored rules no longer validate")
    })?;
    let rules = CompiledRules::compile(sources, &compiler).map_err(|error| {
        tracing::error!(%error, "the stored rules no longer compile");
        internal("the stored rules no longer compile")
    })?;
    let items = deferred_from_plan(&plan).map_err(|error| {
        tracing::error!(%error, "the deferred steps of the plan have the wrong shape");
        internal("the deferred steps are unreadable")
    })?;
    let snapshots = snapshots_from_plan(&plan).map_err(|error| {
        tracing::error!(%error, "the snapshots of the plan have the wrong shape");
        internal("the plan snapshots are unreadable")
    })?;
    let mut deferred = Vec::with_capacity(items.len());
    for item in items {
        let step = deferred_step(&item, &compiler).map_err(Prep::Fail)?;
        let snapshot = snapshots
            .get(item.snapshot)
            .cloned()
            .ok_or_else(|| internal("a deferred step names a missing snapshot"))?;
        deferred.push(Deferred {
            item,
            snapshot,
            step,
        });
    }
    Ok(Compiled {
        rules: Arc::new(rules),
        deferred,
    })
}

fn outbound_steps(rules: &CompiledRules, start: usize, deferred: usize) -> usize {
    let counted = rules
        .rules()
        .iter()
        .flat_map(|rule| rule.steps().iter())
        .skip(start)
        .filter(|step| {
            matches!(
                step.kind().class(),
                StepClass::Fetch | StepClass::SideEffect
            )
        })
        .count();
    counted + deferred
}

fn secret_names(rules: &CompiledRules, deferred: &[Deferred]) -> BTreeSet<String> {
    let mut names = rules.secret_names().clone();
    for entry in deferred {
        if let Action::StoreS3 {
            access_key_secret,
            secret_key_secret,
            ..
        } = &entry.step.action
        {
            names.insert(access_key_secret.clone());
            names.insert(secret_key_secret.clone());
        }
    }
    names
}

async fn open_secrets(
    shared: &Shared,
    endpoint: &EndpointId,
    names: &BTreeSet<String>,
) -> Result<Opened, Prep> {
    let Some(keyring) = &shared.keyring else {
        return Ok(Opened::new(Vec::new()));
    };
    let mut opened: Vec<(SecretName, Secret)> = Vec::with_capacity(names.len());
    for raw in names {
        let Ok(name) = SecretName::parse(raw) else {
            continue;
        };
        let sealed = match shared.store.get_sealed_secret(endpoint, &name).await {
            Ok(Some(sealed)) => sealed,
            Ok(None) => continue,
            Err(error) if error.is_unavailable() => return Err(Prep::Halt),
            Err(error) => {
                tracing::warn!(%error, secret = %name, "a secret cannot be read");
                continue;
            }
        };
        match keyring.open(endpoint, &name, &sealed) {
            Ok(secret) => {
                if secret.unpack().len() < MIN_REDACTED_VALUE_BYTES {
                    return Err(Prep::Fail(Failure::new(
                        FailureCode::Http,
                        format!(
                            "secret {name} is shorter than {MIN_REDACTED_VALUE_BYTES} bytes and cannot be masked in logs"
                        ),
                    )));
                }
                opened.push((name, secret));
            }
            Err(error) => {
                tracing::warn!(%error, secret = %name, "a secret cannot be opened");
            }
        }
    }
    Ok(Opened::new(opened))
}

pub(crate) async fn prepare(shared: &Shared, run: &ClaimedRun) -> Result<Prepared, Prep> {
    let endpoint =
        EndpointId::parse(&run.endpoint_id).map_err(|_| internal("the endpoint id is invalid"))?;
    let capture = CaptureId::parse(&run.capture_id.hyphenated().to_string())
        .map_err(|_| internal("the capture id is invalid"))?;
    let detail = match shared.store.get_capture(&endpoint, &capture).await {
        Ok(Some(detail)) => detail,
        Ok(None) => return Err(internal("the capture no longer exists")),
        Err(error) => {
            tracing::warn!(%error, run_id = %run.id, "the capture of a run cannot be read");
            return Err(Prep::Halt);
        }
    };
    let hop = detail_hop(&detail.headers);
    if hop >= MAX_HOPS {
        return Err(Prep::Fail(Failure::new(
            FailureCode::HopLimit,
            "the capture arrived at the hop limit",
        )));
    }
    let facts = RuleFacts::from_detail(&detail, &shared.settings.public_url, run.title.as_deref())
        .map_err(|_| internal("the stored capture time cannot be read"))?;
    if run.plan.is_null() {
        return Err(internal("the run has no plan"));
    }
    let plan = run.plan.clone();
    let payload = plan_payload(&plan);
    let machine_start = plan_resume_step(&plan);
    let compiler = shared.compiler;
    let compiled = tokio::task::spawn_blocking(move || compile(plan, compiler))
        .await
        .map_err(|_| internal("the rule set job did not finish"))??;
    let Compiled { rules, deferred } = compiled;
    if outbound_steps(&rules, machine_start, deferred.len()) > shared.settings.max_outbound_steps {
        return Err(Prep::Fail(Failure::new(
            FailureCode::Quota,
            format!(
                "the run has more than {} outbound steps",
                shared.settings.max_outbound_steps
            ),
        )));
    }
    let opened = open_secrets(shared, &endpoint, &secret_names(&rules, &deferred)).await?;
    let scrub = Scrubber::new(&opened).map_err(|_| internal("the secrets cannot be masked"))?;
    let log =
        entries_from_value(&run.log).map_err(|_| internal("the stored run log is unreadable"))?;
    let steps = steps_from_value(&run.step_results)
        .map_err(|_| internal("the stored step results are unreadable"))?;
    let mut context = facts.clone().into_context();
    if let Value::Object(vars) = &run.vars {
        context.vars = vars
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
    }
    context.secrets = Secrets::new(
        opened
            .iter()
            .map(|(name, secret)| (name.unpack().to_owned(), secret.unpack().to_owned()))
            .collect(),
    );
    load_rule_variables(
        &shared.store,
        &endpoint,
        &rules,
        &mut context,
        VARIABLES_ACQUIRE,
        VARIABLES_DEADLINE,
    )
    .await
    .map_err(Prep::Fail)?;
    Ok(Prepared {
        endpoint,
        detail,
        hop,
        facts,
        compiled: rules,
        deferred,
        machine_start,
        payload,
        opened,
        scrub,
        context,
        log,
        steps,
    })
}
