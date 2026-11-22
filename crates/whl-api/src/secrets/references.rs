use whl_rules::{CompiledRules, StoredRule, load_stored};
use whl_schedule::CompiledRequest;
use whl_script::{EngineLimits, ScriptCompiler};
use whl_secrets::SecretName;
use whl_store::notify::{Channel, ChannelKind};
use whl_store::rules::RuleRecord;
use whl_store::schedules::ScheduleRecord;
use whl_store::secrets::{ReferenceKind, ReferenceSnapshot, SecretReference, SecretStoreError};

const TELEGRAM_TOKEN_KEY: &str = "token_secret";

pub fn find(
    snapshot: &ReferenceSnapshot,
    name: &SecretName,
) -> Result<Vec<SecretReference>, SecretStoreError> {
    let mut found = rule_references(&snapshot.rules, name)?;
    found.extend(channel_references(&snapshot.channels, name));
    found.extend(schedule_references(&snapshot.schedules, name));
    Ok(found)
}

fn rule_references(
    records: &[RuleRecord],
    name: &SecretName,
) -> Result<Vec<SecretReference>, SecretStoreError> {
    if records.is_empty() {
        return Ok(Vec::new());
    }
    let scripts = ScriptCompiler::new(EngineLimits::default())
        .map_err(|error| SecretStoreError::Invalid(format!("the rule engine: {error}")))?;
    let stored: Vec<StoredRule> = records
        .iter()
        .map(|record| StoredRule {
            id: record.id,
            name: record.name.clone(),
            enabled: true,
            condition: record.condition.clone(),
            steps: record.steps.clone(),
        })
        .collect();
    let sources = load_stored(&stored, &scripts)
        .map_err(|error| SecretStoreError::Invalid(format!("a stored rule: {error}")))?;
    let compiled = CompiledRules::compile(sources, &scripts)
        .map_err(|error| SecretStoreError::Invalid(format!("a stored rule: {error}")))?;
    Ok(compiled
        .rules()
        .iter()
        .filter(|rule| rule.secret_names().contains(name.unpack()))
        .map(|rule| SecretReference {
            kind: ReferenceKind::Rule,
            id: rule.id().hyphenated().to_string(),
            name: Some(rule.name().to_owned()),
        })
        .collect())
}

fn channel_references(channels: &[Channel], name: &SecretName) -> Vec<SecretReference> {
    channels
        .iter()
        .filter(|channel| channel.kind == ChannelKind::Telegram)
        .filter(|channel| {
            channel
                .config
                .get(TELEGRAM_TOKEN_KEY)
                .and_then(serde_json::Value::as_str)
                == Some(name.unpack())
        })
        .map(|channel| SecretReference {
            kind: ReferenceKind::Channel,
            id: channel.id.hyphenated().to_string(),
            name: Some(channel.name.clone()),
        })
        .collect()
}

fn schedule_references(schedules: &[ScheduleRecord], name: &SecretName) -> Vec<SecretReference> {
    schedules
        .iter()
        .filter(|schedule| {
            CompiledRequest::compile(&schedule.request)
                .is_ok_and(|compiled| compiled.secret_names().contains(name.unpack()))
        })
        .map(|schedule| SecretReference {
            kind: ReferenceKind::Schedule,
            id: schedule.id.hyphenated().to_string(),
            name: Some(schedule.name.clone()),
        })
        .collect()
}
