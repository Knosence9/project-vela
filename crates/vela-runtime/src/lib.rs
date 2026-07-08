use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::thread::sleep;
use std::time::Duration;
use vela_config::{BootstrapConfig, ConfigSource, ResolvedConfig};
use vela_extensions::ExtensionsReport;
use vela_memory::MemoryReport;
use vela_review::ReviewReport;
use vela_skills::SkillsReport;
use vela_state::{PersistenceReport, SessionRuntimeReport};

pub use vela_config::preparse_profile_override;
pub use vela_extensions::{
    ExtensionActivation, ExtensionKind, ExtensionLifecycle, ExtensionRecord,
};
pub use vela_state::{
    InteractionMode, RuntimeTurnLifecycleRecord, SessionAction, SessionBranchRecord,
    SessionCompressionRecord, SessionEventRecord, SessionInspection, SessionMessageRecord,
    SessionSearchHit, SessionSummary,
};

mod ops;
mod scheduler;
mod surface;

#[cfg(test)]
mod tests;

pub use ops::*;
pub use scheduler::*;
pub use surface::*;

/// Infers review signals from the latest session and appends them as events.
pub fn emit_review_signals_from_latest_session(
    bootstrap: &BootstrapReport,
    limit: usize,
) -> Result<Option<vela_review::SignalReport>> {
    let Some(session) =
        vela_state::inspect_latest_session(&bootstrap.persistence.state_db_path, limit)?
    else {
        return Ok(None);
    };
    let input = vela_review::SuggestionInput {
        session_id: session.session_id.clone(),
        session_title: session.title.clone(),
        messages: session
            .messages
            .iter()
            .map(|m| vela_review::SuggestionMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect(),
        events: session
            .events
            .iter()
            .map(|e| vela_review::SuggestionEvent {
                event_type: e.event_type.clone(),
                payload_json: e.payload_json.clone(),
            })
            .collect(),
    };
    let report = vela_review::infer_signals(&input)?;
    for signal in &report.signals {
        let logged = vela_state::append_event_to_session(
            &bootstrap.persistence.state_db_path,
            &report.session_id,
            &signal.event_type,
            signal.payload_json.clone(),
        )?;
        if !logged {
            tracing::warn!("failed to append review signal to inspected session");
        }
    }
    let logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &report.session_id,
        "review_signals_emitted",
        json!({
            "session_id": report.session_id,
            "signal_count": report.signals.len(),
            "skipped": report.skipped,
        })
        .to_string(),
    )?;
    if !logged {
        tracing::warn!("failed to append review_signals_emitted event to inspected session");
    }
    Ok(Some(report))
}

mod provider;
pub(crate) use provider::*;
pub use provider::{
    inspect_embedded_lifecycle_guardrails, resolve_runtime_backend_contract,
    supported_runtime_backend_contracts, validate_runtime_backend_config,
    EmbeddedLifecycleGuardrailReport, RuntimeBackendContract, RuntimeProviderCapabilities,
};

fn append_review_event(
    bootstrap: &BootstrapReport,
    session_id: Option<&str>,
    event_type: &str,
    payload_json: String,
) -> Result<()> {
    if let Some(session_id) = session_id {
        let logged = vela_state::append_event_to_session(
            &bootstrap.persistence.state_db_path,
            session_id,
            event_type,
            payload_json,
        )?;
        if !logged {
            tracing::warn!(%session_id, %event_type, "failed to append review event to originating session");
        }
    }
    Ok(())
}

fn load_scheduler_jobs(path: &std::path::Path) -> Result<Vec<ScheduledJob>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&content)?)
}

fn save_scheduler_jobs(path: &std::path::Path, jobs: &[ScheduledJob]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("scheduler jobs path has no parent directory"))?;
    let temp_path = parent.join(format!(
        "{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("jobs.json"),
        unix_timestamp_nanos()
    ));
    std::fs::write(&temp_path, serde_json::to_string_pretty(jobs)?)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

fn acquire_scheduler_jobs_lock(path: &std::path::Path) -> Result<SchedulerJobsLock> {
    let lock_path = path.with_extension("json.lock");
    for _ in 0..100 {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_) => {
                return Ok(SchedulerJobsLock { lock_path });
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => sleep(Duration::from_millis(25)),
            Err(err) => return Err(err.into()),
        }
    }
    bail!("timed out waiting for scheduler jobs lock")
}

struct SchedulerJobsLock {
    lock_path: std::path::PathBuf,
}

impl Drop for SchedulerJobsLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

fn normalize_scheduler_schedule(value: &str) -> Result<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        bail!("scheduler expression cannot be empty");
    }
    let fields = normalized.split_whitespace().collect::<Vec<_>>();
    let supported = matches!(
        fields.as_slice(),
        ["*", "*", "*", "*", "*"] | ["0", "*", "*", "*", "*"] | ["0", "0", "*", "*", "*"]
    ) || matches!(fields.as_slice(), [minute, "*", "*", "*", "*"] if minute.starts_with("*/") && minute[2..].parse::<u32>().ok().is_some_and(|value| value > 0));
    if !supported {
        bail!("unsupported scheduler expression {:?}; supported patterns are '* * * * *', '*/N * * * *', '0 * * * *', and '0 0 * * *'", normalized);
    }
    Ok(normalized)
}

fn normalize_scheduler_task(value: &str) -> Result<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        bail!("scheduled task cannot be empty");
    }
    Ok(normalized.to_string())
}

fn normalize_scheduler_source(source: Option<&str>) -> String {
    source
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("scheduler")
        .to_string()
}

fn normalize_scheduler_delivery_webhook_url(url: Option<&str>) -> Result<Option<String>> {
    let Some(url) = url.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let parsed = reqwest::Url::parse(url)
        .map_err(|err| anyhow::anyhow!("invalid scheduler delivery webhook url: {err}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(Some(parsed.to_string())),
        scheme => bail!(
            "unsupported scheduler delivery webhook url scheme {:?}",
            scheme
        ),
    }
}

fn normalize_scheduler_delivery_event_type(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn validate_scheduler_job_id(id: &str) -> Result<&str> {
    let normalized = id.trim();
    if normalized.is_empty()
        || normalized == "."
        || normalized == ".."
        || normalized.contains('/')
        || normalized.contains('\\')
    {
        bail!("invalid scheduled job id");
    }
    Ok(normalized)
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn unix_timestamp_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Generates review candidates from the latest persisted session.
pub fn generate_review_candidates_from_latest_session(
    bootstrap: &BootstrapReport,
    limit: usize,
) -> Result<Option<vela_review::SuggestionReport>> {
    let Some(session) =
        vela_state::inspect_latest_session(&bootstrap.persistence.state_db_path, limit)?
    else {
        return Ok(None);
    };
    let report = vela_review::generate_candidates(
        &bootstrap.vela_home,
        vela_review::SuggestionInput {
            session_id: session.session_id.clone(),
            session_title: session.title.clone(),
            messages: session
                .messages
                .into_iter()
                .map(|m| vela_review::SuggestionMessage {
                    role: m.role,
                    content: m.content,
                })
                .collect(),
            events: session
                .events
                .into_iter()
                .map(|e| vela_review::SuggestionEvent {
                    event_type: e.event_type,
                    payload_json: e.payload_json,
                })
                .collect(),
        },
    )?;
    for candidate_id in &report.candidate_ids {
        let candidate = vela_review::get_candidate(&bootstrap.vela_home, candidate_id)?;
        append_review_event(
            bootstrap,
            candidate.origin_session_id.as_deref(),
            "review_candidate_created",
            json!({
                "candidate_id": candidate.id,
                "kind": candidate.kind.label(),
                "source": candidate.source,
                "reason": candidate.reason,
                "origin_session_id": candidate.origin_session_id.clone(),
                "origin_session_title": candidate.origin_session_title.clone(),
            })
            .to_string(),
        )?;
    }
    let logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &report.session_id,
        "review_candidates_generated",
        json!({
            "session_id": report.session_id,
            "candidate_ids": report.candidate_ids,
            "skipped": report.skipped,
        })
        .to_string(),
    )?;
    if !logged {
        tracing::warn!("failed to append review_candidates_generated event to inspected session");
    }
    Ok(Some(report))
}

impl BootstrapReport {
    fn from_parts(
        config: BootstrapConfig,
        persistence: PersistenceReport,
        memory: MemoryReport,
        skills: SkillsReport,
        reviews: ReviewReport,
        extensions: ExtensionsReport,
    ) -> Self {
        Self {
            vela_home: config.vela_home,
            active_profile: config.active_profile,
            loaded_env_paths: config.loaded_env_paths,
            ignored_user_config: config.ignored_user_config,
            config_sources: config.config_sources,
            resolved_config: config.resolved_config,
            persistence,
            memory,
            skills,
            reviews,
            extensions,
        }
    }
}

pub use vela_memory::{MemoryTarget, MEMORY_CHAR_LIMIT, USER_CHAR_LIMIT};
pub use vela_state::SessionRequest;
