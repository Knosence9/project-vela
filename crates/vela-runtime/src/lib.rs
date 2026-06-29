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
use vela_memory::MemoryReport;
use vela_review::ReviewReport;
use vela_skills::SkillsReport;
use vela_state::{PersistenceReport, SessionRuntimeReport};

pub use vela_config::preparse_profile_override;
pub use vela_state::{
    InteractionMode, SessionAction, SessionEventRecord, SessionInspection, SessionMessageRecord,
    SessionSearchHit, SessionSummary,
};

#[derive(Debug, Clone)]
/// Describes the durable gateway paths ensured during bootstrap.
pub struct GatewaySetupReport {
    pub gateway_dir: std::path::PathBuf,
    pub config_path: std::path::PathBuf,
    pub inbox_dir: std::path::PathBuf,
    pub outbox_dir: std::path::PathBuf,
    pub config_existed_before: bool,
}

#[derive(Debug, Clone)]
/// Captures gateway setup data plus the resolved runtime session.
pub struct GatewayStartReport {
    pub setup: GatewaySetupReport,
    pub session: SessionRuntimeReport,
}

#[derive(Debug, Clone)]
/// Describes the durable scheduler files ensured during bootstrap.
pub struct SchedulerSetupReport {
    pub scheduler_dir: std::path::PathBuf,
    pub config_path: std::path::PathBuf,
    pub jobs_path: std::path::PathBuf,
    pub config_existed_before: bool,
    pub jobs_existed_before: bool,
    pub job_count: usize,
}

#[derive(Debug, Clone)]
/// Captures scheduler setup data plus the resolved runtime session.
pub struct SchedulerStartReport {
    pub setup: SchedulerSetupReport,
    pub session: SessionRuntimeReport,
}

#[derive(Debug, Clone)]
/// Captures one executed chat/runtime turn plus any operator-triggered review work.
pub struct ChatTurnReport {
    pub session: SessionRuntimeReport,
    pub response: Option<String>,
    pub response_source: String,
    pub emitted_signal_count: usize,
    pub generated_candidate_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents one durable scheduler registration stored in `jobs.json`.
pub struct ScheduledJob {
    pub id: String,
    pub schedule: String,
    pub task: String,
    pub source: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
/// Aggregates the initialized runtime subsystems and resolved configuration.
pub struct BootstrapReport {
    pub vela_home: std::path::PathBuf,
    pub active_profile: Option<String>,
    pub loaded_env_paths: Vec<std::path::PathBuf>,
    pub ignored_user_config: bool,
    pub config_sources: Vec<ConfigSource>,
    pub resolved_config: ResolvedConfig,
    pub persistence: PersistenceReport,
    pub memory: MemoryReport,
    pub skills: SkillsReport,
    pub reviews: ReviewReport,
}

impl BootstrapReport {
    /// Renders a compact human-readable bootstrap summary.
    pub fn summary_line(&self) -> String {
        let profile = self
            .active_profile
            .as_deref()
            .map(|p| format!(" profile={p}"))
            .unwrap_or_default();
        let env_count = self.loaded_env_paths.len();
        let config_count = self
            .config_sources
            .iter()
            .filter(|source| {
                matches!(
                    source.kind,
                    vela_config::ConfigSourceKind::User | vela_config::ConfigSourceKind::ProjectFallback
                )
            })
            .count();
        format!(
            "vela bootstrap ready: home={} env_files={} config_files={} ignore_user_config={} state_db_runs={}{}",
            self.vela_home.display(),
            env_count,
            config_count,
            self.ignored_user_config,
            self.persistence.bootstrap_runs,
            profile
        )
    }
}

/// Initializes config, persistence, memory, skills, and review subsystems.
pub fn initialize_bootstrap(active_profile: Option<String>, ignore_user_config: bool) -> Result<BootstrapReport> {
    let config = vela_config::initialize_config(active_profile, ignore_user_config)?;
    let persistence = vela_state::initialize_persistence(&config.vela_home)?;
    let memory = vela_memory::initialize_memory(&config.vela_home)?;
    let skills = vela_skills::initialize_skills(&config.vela_home)?;
    let reviews = vela_review::initialize_reviews(&config.vela_home)?;
    Ok(BootstrapReport::from_parts(config, persistence, memory, skills, reviews))
}

/// Emits a debug log once runtime bootstrap has completed.
pub fn bootstrap_banner() {
    tracing::debug!("vela-runtime bootstrap initialized");
}

/// Returns the current session id and title when one is active.
pub fn current_session_identity(bootstrap: &BootstrapReport) -> Result<Option<(String, String)>> {
    vela_state::current_session_identity(&bootstrap.persistence.state_db_path)
}

/// Returns the latest session summary when one exists.
pub fn current_session_summary(bootstrap: &BootstrapReport) -> Result<Option<SessionSummary>> {
    vela_state::current_session_summary(&bootstrap.persistence.state_db_path)
}

/// Returns the latest session summary for a command-scoped runtime session.
pub fn current_command_session_summary(
    bootstrap: &BootstrapReport,
    command_name: &str,
) -> Result<Option<SessionSummary>> {
    vela_state::current_command_session_summary(&bootstrap.persistence.state_db_path, command_name)
}

/// Resolves or creates a runtime session for an interactive request.
pub fn resolve_runtime_session(bootstrap: &BootstrapReport, request: &SessionRequest) -> Result<SessionRuntimeReport> {
    vela_state::resolve_runtime_session(&bootstrap.persistence.state_db_path, request)
}

/// Ensures the durable gateway directory structure and config file exist.
pub fn setup_gateway(bootstrap: &BootstrapReport) -> Result<GatewaySetupReport> {
    let gateway_dir = bootstrap.vela_home.join("gateway");
    std::fs::create_dir_all(&gateway_dir)?;
    let inbox_dir = gateway_dir.join("inbox");
    let outbox_dir = gateway_dir.join("outbox");
    std::fs::create_dir_all(&inbox_dir)?;
    std::fs::create_dir_all(&outbox_dir)?;

    let config_path = gateway_dir.join("config.json");
    let config_existed_before = config_path.is_file();
    if !config_existed_before {
        std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "default_source": "gateway",
                "session_command_name": "gateway",
                "active_profile": bootstrap.active_profile,
                "display_interface": bootstrap.resolved_config.display_interface,
                "transport_mode": "local-bootstrap",
            }))?,
        )?;
    }

    Ok(GatewaySetupReport {
        gateway_dir,
        config_path,
        inbox_dir,
        outbox_dir,
        config_existed_before,
    })
}

/// Starts or resumes the durable gateway runtime session.
pub fn start_gateway(bootstrap: &BootstrapReport) -> Result<GatewayStartReport> {
    let setup = setup_gateway(bootstrap)?;
    let session = vela_state::resolve_command_session(
        &bootstrap.persistence.state_db_path,
        "gateway",
        InteractionMode::Interactive,
    )?;
    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
        "gateway_started",
        json!({
            "source": "gateway",
            "config_path": setup.config_path,
            "inbox_dir": setup.inbox_dir,
            "outbox_dir": setup.outbox_dir,
            "action": session.action.label(),
        })
        .to_string(),
    )?;
    if !event_logged {
        tracing::warn!(session_id=%session.session_id, "failed to append gateway_started event");
    }
    if matches!(session.action, SessionAction::Created) {
        let message_logged = vela_state::append_message_to_session(
            &bootstrap.persistence.state_db_path,
            &session.session_id,
            "system",
            "Gateway bootstrap ready.",
            Some(
                json!({
                    "source": "gateway",
                    "direction": "egress",
                    "config_path": setup.config_path,
                })
                .to_string(),
            ),
        )?;
        if !message_logged {
            tracing::warn!(session_id=%session.session_id, "failed to append gateway bootstrap message");
        }
    }
    Ok(GatewayStartReport { setup, session })
}

/// Ensures the durable scheduler directory, config, and job registry exist.
pub fn setup_scheduler(bootstrap: &BootstrapReport) -> Result<SchedulerSetupReport> {
    let scheduler_dir = bootstrap.vela_home.join("scheduler");
    std::fs::create_dir_all(&scheduler_dir)?;

    let config_path = scheduler_dir.join("config.json");
    let jobs_path = scheduler_dir.join("jobs.json");
    let config_existed_before = config_path.is_file();
    let jobs_existed_before = jobs_path.is_file();

    if !config_existed_before {
        std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "default_source": "scheduler",
                "session_command_name": "cron",
                "active_profile": bootstrap.active_profile,
                "transport_mode": "local-bootstrap",
            }))?,
        )?;
    }
    if !jobs_existed_before {
        std::fs::write(&jobs_path, "[]")?;
    }

    let job_count = load_scheduler_jobs(&jobs_path)?.len();
    Ok(SchedulerSetupReport {
        scheduler_dir,
        config_path,
        jobs_path,
        config_existed_before,
        jobs_existed_before,
        job_count,
    })
}

/// Starts or resumes the durable scheduler runtime session.
pub fn start_scheduler(bootstrap: &BootstrapReport) -> Result<SchedulerStartReport> {
    let setup = setup_scheduler(bootstrap)?;
    let session = vela_state::resolve_command_session(
        &bootstrap.persistence.state_db_path,
        "cron",
        InteractionMode::Interactive,
    )?;
    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
        "scheduler_started",
        json!({
            "source": "scheduler",
            "config_path": setup.config_path,
            "jobs_path": setup.jobs_path,
            "job_count": setup.job_count,
            "action": session.action.label(),
        })
        .to_string(),
    )?;
    if !event_logged {
        tracing::warn!(session_id=%session.session_id, "failed to append scheduler_started event");
    }
    if matches!(session.action, SessionAction::Created) {
        let message_logged = vela_state::append_message_to_session(
            &bootstrap.persistence.state_db_path,
            &session.session_id,
            "system",
            "Scheduler bootstrap ready.",
            Some(
                json!({
                    "source": "scheduler",
                    "direction": "egress",
                    "config_path": setup.config_path,
                    "jobs_path": setup.jobs_path,
                })
                .to_string(),
            ),
        )?;
        if !message_logged {
            tracing::warn!(session_id=%session.session_id, "failed to append scheduler bootstrap message");
        }
    }
    Ok(SchedulerStartReport { setup, session })
}

/// Executes one runtime turn, optionally emitting review/checkpoint artifacts.
pub fn execute_chat_turn(
    bootstrap: &BootstrapReport,
    request: &SessionRequest,
    provider_override: Option<&str>,
    model_override: Option<&str>,
    checkpoints: bool,
) -> Result<ChatTurnReport> {
    let session = resolve_runtime_session(bootstrap, request)?;
    let rendered = render_chat_response(bootstrap, &session, request, provider_override, model_override)?;

    if let Some(content) = rendered.content.as_deref() {
        let logged = vela_state::append_message_to_session(
            &bootstrap.persistence.state_db_path,
            &session.session_id,
            "assistant",
            content,
            Some(
                json!({
                    "source": rendered.source,
                    "provider": rendered.provider,
                    "model": rendered.model,
                    "checkpoints": checkpoints,
                    "interaction_mode": session.interaction_mode.label(),
                })
                .to_string(),
            ),
        )?;
        if !logged {
            tracing::warn!(session_id=%session.session_id, "failed to append assistant runtime response");
        }
    }

    let mut emitted_signal_count = 0usize;
    let mut generated_candidate_count = 0usize;
    if checkpoints {
        if let Some(report) = emit_review_signals_from_latest_session(bootstrap, 50)? {
            emitted_signal_count = report.signals.len();
        }
        if let Some(report) = generate_review_candidates_from_latest_session(bootstrap, 50)? {
            generated_candidate_count = report.candidate_ids.len();
        }
    }

    Ok(ChatTurnReport {
        session,
        response: rendered.content,
        response_source: rendered.source.to_string(),
        emitted_signal_count,
        generated_candidate_count,
    })
}

/// Lists all durable scheduled jobs currently registered.
pub fn list_scheduled_jobs(bootstrap: &BootstrapReport) -> Result<Vec<ScheduledJob>> {
    let setup = setup_scheduler(bootstrap)?;
    load_scheduler_jobs(&setup.jobs_path)
}

/// Loads one scheduled job by id from the durable registry.
pub fn get_scheduled_job(bootstrap: &BootstrapReport, id: &str) -> Result<ScheduledJob> {
    let job_id = validate_scheduler_job_id(id)?;
    list_scheduled_jobs(bootstrap)?
        .into_iter()
        .find(|job| job.id == job_id)
        .ok_or_else(|| anyhow::anyhow!("scheduled job {:?} not found", job_id))
}

/// Registers a new durable scheduled job after validation and deduplication.
pub fn add_scheduled_job(
    bootstrap: &BootstrapReport,
    schedule: &str,
    task: &str,
    source: Option<&str>,
) -> Result<ScheduledJob> {
    let setup = setup_scheduler(bootstrap)?;
    let schedule = normalize_scheduler_schedule(schedule)?;
    let task = normalize_scheduler_task(task)?;
    let source = normalize_scheduler_source(source);
    let lock = acquire_scheduler_jobs_lock(&setup.jobs_path)?;
    let mut jobs = load_scheduler_jobs(&setup.jobs_path)?;
    if jobs.iter().any(|job| job.schedule == schedule && job.task == task && job.source == source && job.status == "pending") {
        drop(lock);
        bail!("matching scheduled job is already registered");
    }
    let job = ScheduledJob {
        id: format!("job-{}", unix_timestamp_nanos()),
        schedule,
        task,
        source,
        status: "pending".to_string(),
        created_at: unix_timestamp(),
    };
    jobs.push(job.clone());
    save_scheduler_jobs(&setup.jobs_path, &jobs)?;
    drop(lock);

    if let Some(session) = current_command_session_summary(bootstrap, "cron")? {
        let event_logged = vela_state::append_event_to_session(
            &bootstrap.persistence.state_db_path,
            &session.id,
            "scheduler_job_registered",
            serde_json::to_string(&job)?,
        )?;
        if !event_logged {
            tracing::warn!(session_id=%session.id, job_id=%job.id, "failed to append scheduler job event");
        }
    }
    Ok(job)
}

/// Searches persisted session history using the state FTS index.
pub fn search_session_history(bootstrap: &BootstrapReport, query: &str, limit: usize) -> Result<Vec<SessionSearchHit>> {
    vela_state::search_session_history(&bootstrap.persistence.state_db_path, query, limit)
}

/// Inspects the latest persisted session with recent messages and events.
pub fn inspect_latest_session(bootstrap: &BootstrapReport, limit: usize) -> Result<Option<SessionInspection>> {
    vela_state::inspect_latest_session(&bootstrap.persistence.state_db_path, limit)
}

/// Renders the always-on memory snapshot used for prompting.
pub fn render_memory_snapshot(bootstrap: &BootstrapReport) -> Result<String> {
    vela_memory::render_prompt_snapshot(&bootstrap.vela_home)
}

/// Views the current durable memory contents for a target file.
pub fn view_memory(bootstrap: &BootstrapReport, target: vela_memory::MemoryTarget) -> Result<vela_memory::MemoryView> {
    vela_memory::view_memory(&bootstrap.vela_home, target)
}

/// Appends a memory entry directly to durable memory storage.
pub fn add_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    content: &str,
) -> Result<vela_memory::MemoryMutationReport> {
    vela_memory::add_memory_entry(&bootstrap.vela_home, target, content)
}

/// Stages a memory add for later approval.
pub fn stage_add_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    content: &str,
) -> Result<vela_memory::PendingMemoryWrite> {
    vela_memory::stage_add_memory_entry(&bootstrap.vela_home, target, content)
}

/// Replaces a matching durable memory entry immediately.
pub fn replace_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    old_text: &str,
    content: &str,
) -> Result<vela_memory::MemoryMutationReport> {
    vela_memory::replace_memory_entry(&bootstrap.vela_home, target, old_text, content)
}

/// Stages a memory replacement for later approval.
pub fn stage_replace_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    old_text: &str,
    content: &str,
) -> Result<vela_memory::PendingMemoryWrite> {
    vela_memory::stage_replace_memory_entry(&bootstrap.vela_home, target, old_text, content)
}

/// Removes a matching durable memory entry immediately.
pub fn remove_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    old_text: &str,
) -> Result<vela_memory::MemoryMutationReport> {
    vela_memory::remove_memory_entry(&bootstrap.vela_home, target, old_text)
}

/// Stages a memory removal for later approval.
pub fn stage_remove_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    old_text: &str,
) -> Result<vela_memory::PendingMemoryWrite> {
    vela_memory::stage_remove_memory_entry(&bootstrap.vela_home, target, old_text)
}

/// Lists staged memory writes awaiting approval.
pub fn list_pending_memory(bootstrap: &BootstrapReport) -> Result<Vec<vela_memory::PendingMemoryWrite>> {
    vela_memory::list_pending(&bootstrap.vela_home)
}

/// Loads one staged memory write by id.
pub fn get_pending_memory(bootstrap: &BootstrapReport, id: &str) -> Result<vela_memory::PendingMemoryWrite> {
    vela_memory::get_pending(&bootstrap.vela_home, id)
}

/// Approves and applies one staged memory write.
pub fn approve_pending_memory(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_memory::MemoryMutationReport> {
    vela_memory::approve_pending(&bootstrap.vela_home, id)
}

/// Rejects and deletes one staged memory write.
pub fn reject_pending_memory(bootstrap: &BootstrapReport, id: &str) -> Result<()> {
    vela_memory::reject_pending(&bootstrap.vela_home, id)
}

/// Lists durable skills available in the local skill store.
pub fn list_skills(bootstrap: &BootstrapReport) -> Result<Vec<vela_skills::SkillSummary>> {
    vela_skills::list_skills(&bootstrap.vela_home)
}

/// Loads one durable skill by name.
pub fn view_skill(bootstrap: &BootstrapReport, name: &str) -> Result<vela_skills::SkillView> {
    vela_skills::view_skill(&bootstrap.vela_home, name)
}

/// Creates a durable skill immediately.
pub fn create_skill(
    bootstrap: &BootstrapReport,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<vela_skills::SkillMutationReport> {
    vela_skills::create_skill(&bootstrap.vela_home, name, description, body)
}

/// Stages creation of a durable skill for later approval.
pub fn stage_create_skill(
    bootstrap: &BootstrapReport,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<vela_skills::PendingSkillWrite> {
    vela_skills::stage_create_skill(&bootstrap.vela_home, name, description, body)
}

/// Rewrites an existing durable skill immediately.
pub fn write_skill(
    bootstrap: &BootstrapReport,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<vela_skills::SkillMutationReport> {
    vela_skills::write_skill(&bootstrap.vela_home, name, description, body)
}

/// Stages a durable skill rewrite for later approval.
pub fn stage_write_skill(
    bootstrap: &BootstrapReport,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<vela_skills::PendingSkillWrite> {
    vela_skills::stage_write_skill(&bootstrap.vela_home, name, description, body)
}

/// Deletes a durable skill immediately.
pub fn delete_skill(bootstrap: &BootstrapReport, name: &str) -> Result<vela_skills::SkillMutationReport> {
    vela_skills::delete_skill(&bootstrap.vela_home, name)
}

/// Stages deletion of a durable skill for later approval.
pub fn stage_delete_skill(bootstrap: &BootstrapReport, name: &str) -> Result<vela_skills::PendingSkillWrite> {
    vela_skills::stage_delete_skill(&bootstrap.vela_home, name)
}

/// Lists staged skill writes awaiting approval.
pub fn list_pending_skills(bootstrap: &BootstrapReport) -> Result<Vec<vela_skills::PendingSkillWrite>> {
    vela_skills::list_pending(&bootstrap.vela_home)
}

/// Loads one staged skill write by id.
pub fn get_pending_skill(bootstrap: &BootstrapReport, id: &str) -> Result<vela_skills::PendingSkillWrite> {
    vela_skills::get_pending(&bootstrap.vela_home, id)
}

/// Approves and applies one staged skill write.
pub fn approve_pending_skill(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_skills::SkillMutationReport> {
    vela_skills::approve_pending(&bootstrap.vela_home, id)
}

/// Rejects and deletes one staged skill write.
pub fn reject_pending_skill(bootstrap: &BootstrapReport, id: &str) -> Result<()> {
    vela_skills::reject_pending(&bootstrap.vela_home, id)
}

/// Lists queued review candidates derived from user or background signals.
pub fn list_review_candidates(bootstrap: &BootstrapReport) -> Result<Vec<vela_review::ReviewCandidate>> {
    vela_review::list_candidates(&bootstrap.vela_home)
}

/// Loads one review candidate by id.
pub fn get_review_candidate(bootstrap: &BootstrapReport, id: &str) -> Result<vela_review::ReviewCandidate> {
    vela_review::get_candidate(&bootstrap.vela_home, id)
}

/// Creates a review candidate for a proposed memory mutation.
pub fn stage_memory_review_candidate(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    action: &str,
    old_text: Option<&str>,
    new_text: Option<&str>,
    reason: &str,
    source: Option<&str>,
) -> Result<vela_review::ReviewCandidate> {
    let origin = vela_state::current_session_identity(&bootstrap.persistence.state_db_path)?;
    let candidate = vela_review::stage_memory_candidate(
        &bootstrap.vela_home,
        target,
        action,
        old_text,
        new_text,
        reason,
        source,
        origin.as_ref().map(|(id, _)| id.as_str()),
        origin.as_ref().map(|(_, title)| title.as_str()),
    )?;
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
    Ok(candidate)
}

/// Creates a review candidate for a proposed skill mutation.
pub fn stage_skill_review_candidate(
    bootstrap: &BootstrapReport,
    action: &str,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
    reason: &str,
    source: Option<&str>,
) -> Result<vela_review::ReviewCandidate> {
    let origin = vela_state::current_session_identity(&bootstrap.persistence.state_db_path)?;
    let candidate = vela_review::stage_skill_candidate(
        &bootstrap.vela_home,
        action,
        name,
        description,
        body,
        reason,
        source,
        origin.as_ref().map(|(id, _)| id.as_str()),
        origin.as_ref().map(|(_, title)| title.as_str()),
    )?;
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
    Ok(candidate)
}

/// Promotes a review candidate into the appropriate pending approval queue.
pub fn promote_review_candidate(bootstrap: &BootstrapReport, id: &str) -> Result<vela_review::PromotionReport> {
    let candidate = vela_review::get_candidate(&bootstrap.vela_home, id)?;
    let report = vela_review::promote_candidate(&bootstrap.vela_home, id)?;
    append_review_event(
        bootstrap,
        candidate.origin_session_id.as_deref(),
        "review_candidate_promoted",
        json!({
            "candidate_id": report.candidate_id,
            "kind": report.kind.label(),
            "pending_id": report.pending_id,
            "origin_session_id": candidate.origin_session_id.clone(),
            "origin_session_title": candidate.origin_session_title.clone(),
        })
        .to_string(),
    )?;
    Ok(report)
}

/// Rejects a queued review candidate.
pub fn reject_review_candidate(bootstrap: &BootstrapReport, id: &str) -> Result<()> {
    let candidate = vela_review::reject_candidate(&bootstrap.vela_home, id)?;
    append_review_event(
        bootstrap,
        candidate.origin_session_id.as_deref(),
        "review_candidate_rejected",
        json!({
            "candidate_id": id,
            "origin_session_id": candidate.origin_session_id.clone(),
            "origin_session_title": candidate.origin_session_title.clone(),
        })
        .to_string(),
    )?;
    Ok(())
}

/// Infers review signals from the latest session and appends them as events.
pub fn emit_review_signals_from_latest_session(
    bootstrap: &BootstrapReport,
    limit: usize,
) -> Result<Option<vela_review::SignalReport>> {
    let Some(session) = vela_state::inspect_latest_session(&bootstrap.persistence.state_db_path, limit)? else {
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

struct RenderedChatResponse {
    content: Option<String>,
    source: &'static str,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum RuntimeProviderChoice {
    Ollama,
}

#[derive(Debug, Clone, Copy)]
enum RuntimeToolName {
    MemorySnapshot,
    ListSkills,
}

impl RuntimeToolName {
    fn as_str(self) -> &'static str {
        match self {
            Self::MemorySnapshot => "memory_snapshot",
            Self::ListSkills => "list_skills",
        }
    }
}

const MAX_RUNTIME_TOOL_STEPS: usize = 3;

#[derive(Debug, Clone)]
struct RuntimeExecutionConfig {
    provider: Option<RuntimeProviderChoice>,
    provider_label: Option<String>,
    model: Option<String>,
    ollama_base_url: String,
}

#[derive(Debug, Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeToolRequest {
    tool: String,
}

fn render_chat_response(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    request: &SessionRequest,
    provider_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<RenderedChatResponse> {
    let execution = resolve_runtime_execution(&bootstrap.resolved_config, provider_override, model_override)?;
    if let Some(RuntimeProviderChoice::Ollama) = execution.provider.as_ref() {
        validate_ollama_base_url(&execution.ollama_base_url)?;
    }

    let memory = vela_memory::render_prompt_snapshot(&bootstrap.vela_home)?;
    let skills = vela_skills::list_skills(&bootstrap.vela_home)?;
    let reviews = vela_review::list_candidates(&bootstrap.vela_home)?;
    let memory_lines = memory.lines().count();

    if request.image_present {
        let image_path = request.image_path.as_deref().unwrap_or("(unspecified image path)");
        if let Some(RuntimeProviderChoice::Ollama) = execution.provider.as_ref() {
            if let Some(image_path) = request.image_path.as_deref() {
                let model = execution
                    .model
                    .as_deref()
                    .context("runtime provider 'ollama' requires a model (for example a Gemma family model)")?;
                let image_base64 = encode_image_as_base64(image_path)?;
                let user_prompt = request
                    .query_text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| "Please analyze the attached image and respond concisely with the most relevant details for the runtime session.".to_string());
                let prompt = format!(
                    "You are Vela, a Rust-first agentic OS kernel runtime.\n\nSession: {} ({})\nMemory snapshot:\n{}\n\nLoaded skills: {}\nPending review candidates: {}\n\nUser image request:\n{}\n\nAttached image name: {}\n\nSupported runtime tools:\n- memory_snapshot\n- list_skills\nIf you need one tool before answering, respond with ONLY JSON like {{\"tool\":\"memory_snapshot\"}} or {{\"tool\":\"list_skills\"}}. Otherwise answer directly.",
                    session.title,
                    session.session_id,
                    memory,
                    skills.len(),
                    reviews.len(),
                    user_prompt,
                    std::path::Path::new(image_path).file_name().and_then(|n| n.to_str()).unwrap_or("attachment"),
                );
                return execute_ollama_turn(
                    bootstrap,
                    session,
                    &execution,
                    model,
                    &prompt,
                    Some(vec![image_base64]),
                    &memory,
                    &skills,
                );
            }
        }

        return Ok(RenderedChatResponse {
            content: Some(format!(
                "Vela executed a local image turn.\n\nImage: {}\nSession: {} ({})\nMemory snapshot lines: {}\nLoaded skills: {}\nPending review candidates: {}\n\nNo provider-backed image execution was available, so this deterministic local-kernel scaffold response kept persistence, review, and continuity live.",
                image_path,
                session.title,
                session.session_id,
                memory_lines,
                skills.len(),
                reviews.len(),
            )),
            source: "runtime-kernel",
            provider: execution.provider_label,
            model: execution.model,
        });
    }

    if let Some(query) = request.query_text.as_deref() {
        if let Some(RuntimeProviderChoice::Ollama) = execution.provider.as_ref() {
            let model = execution
                .model
                .as_deref()
                .context("runtime provider 'ollama' requires a model (for example a Gemma family model)")?;
            let prompt = format!(
                "You are Vela, a Rust-first agentic OS kernel runtime.\n\nSession: {} ({})\nMemory snapshot:\n{}\n\nLoaded skills: {}\nPending review candidates: {}\n\nUser query:\n{}\n\nSupported runtime tools:\n- memory_snapshot\n- list_skills\nIf you need one tool before answering, respond with ONLY JSON like {{\"tool\":\"memory_snapshot\"}} or {{\"tool\":\"list_skills\"}}. Otherwise answer directly.",
                session.title,
                session.session_id,
                memory,
                skills.len(),
                reviews.len(),
                query.trim(),
            );
            return execute_ollama_turn(
                bootstrap,
                session,
                &execution,
                model,
                &prompt,
                None,
                &memory,
                &skills,
            );
        }

        return Ok(RenderedChatResponse {
            content: Some(format!(
                "Vela executed a local kernel turn.\n\nQuery: {}\nSession: {} ({})\nMemory snapshot lines: {}\nLoaded skills: {}\nPending review candidates: {}\n\nNo provider-backed execution was available, so this deterministic local-kernel scaffold response kept persistence, review, and continuity live.",
                query.trim(),
                session.title,
                session.session_id,
                memory_lines,
                skills.len(),
                reviews.len(),
            )),
            source: "runtime-kernel",
            provider: None,
            model: None,
        });
    }

    if matches!(session.action, SessionAction::Created) {
        return Ok(RenderedChatResponse {
            content: Some(format!(
                "Interactive Vela runtime ready. Session: {} ({}). Loaded skills: {}. Pending review candidates: {}.",
                session.title,
                session.session_id,
                skills.len(),
                reviews.len(),
            )),
            source: "runtime-kernel",
            provider: execution.provider_label,
            model: execution.model,
        });
    }

    Ok(RenderedChatResponse {
        content: None,
        source: "runtime-kernel",
        provider: execution.provider_label,
        model: execution.model,
    })
}

fn resolve_runtime_execution(
    resolved: &ResolvedConfig,
    provider_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<RuntimeExecutionConfig> {
    let provider_label = provider_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .or_else(|| resolved.runtime_provider.as_ref().map(|s| s.trim().to_ascii_lowercase()));
    let provider = match provider_label.as_deref() {
        Some("ollama") => Some(RuntimeProviderChoice::Ollama),
        Some(other) => bail!("unsupported runtime provider {other:?}"),
        None => None,
    };
    let model = model_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| resolved.runtime_model.clone());
    let ollama_base_url = resolved
        .runtime_ollama_base_url
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());

    Ok(RuntimeExecutionConfig {
        provider,
        provider_label,
        model,
        ollama_base_url,
    })
}

/// Executes one provider-backed Ollama turn and optionally completes a bounded local tool loop.
fn execute_ollama_turn(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    execution: &RuntimeExecutionConfig,
    model: &str,
    prompt: &str,
    images: Option<Vec<String>>,
    memory: &str,
    skills: &[vela_skills::SkillSummary],
) -> Result<RenderedChatResponse> {
    let mut current_prompt = prompt.to_string();
    let mut used_tool_loop = false;

    for step in 1..=MAX_RUNTIME_TOOL_STEPS {
        let response = call_ollama_generate(&execution.ollama_base_url, model, &current_prompt, images.clone())?;
        if let Some(tool_name) = parse_runtime_tool_request(&response)? {
            used_tool_loop = true;
            persist_runtime_tool_request(bootstrap, &session.session_id, tool_name, step)?;
            let tool_result = execute_runtime_tool(tool_name, memory, skills);
            persist_runtime_tool_result(bootstrap, &session.session_id, tool_name, step, &tool_result)?;

            let followup_instruction = if step == MAX_RUNTIME_TOOL_STEPS {
                "You have reached the maximum number of tool steps. Answer the user directly without requesting another tool."
            } else {
                "You may either request another supported tool with ONLY JSON like {\"tool\":\"memory_snapshot\"} or {\"tool\":\"list_skills\"}, or answer directly."
            };
            current_prompt = format!(
                "{}\n\nCompleted tool step {} of {}.\nTool result for {}:\n{}\n\n{}",
                current_prompt,
                step,
                MAX_RUNTIME_TOOL_STEPS,
                tool_name.as_str(),
                tool_result,
                followup_instruction,
            );
            continue;
        }

        return Ok(RenderedChatResponse {
            content: Some(response),
            source: if used_tool_loop { "runtime-ollama-tool-loop" } else { "runtime-ollama" },
            provider: execution.provider_label.clone(),
            model: execution.model.clone(),
        });
    }

    let final_response = call_ollama_generate(&execution.ollama_base_url, model, &current_prompt, images)?;
    if parse_runtime_tool_request(&final_response)?.is_some() {
        return Ok(RenderedChatResponse {
            content: Some("Vela reached the maximum bounded tool steps and fell back to a deterministic runtime response instead of continuing indefinitely.".to_string()),
            source: "runtime-kernel",
            provider: execution.provider_label.clone(),
            model: execution.model.clone(),
        });
    }

    Ok(RenderedChatResponse {
        content: Some(final_response),
        source: "runtime-ollama-tool-loop",
        provider: execution.provider_label.clone(),
        model: execution.model.clone(),
    })
}

/// Parses a model response for a supported runtime tool request envelope.
fn parse_runtime_tool_request(response: &str) -> Result<Option<RuntimeToolName>> {
    let trimmed = response.trim();
    let json_body = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let Ok(request) = serde_json::from_str::<RuntimeToolRequest>(json_body) else {
        return Ok(None);
    };
    let tool = match request.tool.trim() {
        "memory_snapshot" => RuntimeToolName::MemorySnapshot,
        "list_skills" => RuntimeToolName::ListSkills,
        other => bail!("unsupported runtime tool request {:?}", other),
    };
    Ok(Some(tool))
}

/// Executes one approved read-only runtime tool and returns its textual result.
fn execute_runtime_tool(tool: RuntimeToolName, memory: &str, skills: &[vela_skills::SkillSummary]) -> String {
    match tool {
        RuntimeToolName::MemorySnapshot => memory.to_string(),
        RuntimeToolName::ListSkills => {
            if skills.is_empty() {
                "(no loaded skills)".to_string()
            } else {
                skills
                    .iter()
                    .map(|skill| skill.name.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    }
}

/// Persists the requested runtime tool before execution begins.
fn persist_runtime_tool_request(bootstrap: &BootstrapReport, session_id: &str, tool: RuntimeToolName, step: usize) -> Result<()> {
    let metadata = json!({"source": "runtime-tool-loop", "tool": tool.as_str(), "step": step}).to_string();
    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "runtime_tool_requested",
        metadata.clone(),
    )?;
    if !event_logged {
        bail!("failed to persist runtime tool request event for session {:?}", session_id);
    }
    let message_logged = vela_state::append_message_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "tool-request",
        tool.as_str(),
        Some(metadata),
    )?;
    if !message_logged {
        bail!("failed to persist runtime tool request message for session {:?}", session_id);
    }
    Ok(())
}

/// Persists the completed runtime tool result and its metadata.
fn persist_runtime_tool_result(bootstrap: &BootstrapReport, session_id: &str, tool: RuntimeToolName, step: usize, result: &str) -> Result<()> {
    let metadata = json!({"source": "runtime-tool-loop", "tool": tool.as_str(), "step": step, "result_length": result.len()}).to_string();
    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "runtime_tool_completed",
        metadata.clone(),
    )?;
    if !event_logged {
        bail!("failed to persist runtime tool completion event for session {:?}", session_id);
    }
    let message_logged = vela_state::append_message_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "tool-result",
        result,
        Some(metadata),
    )?;
    if !message_logged {
        bail!("failed to persist runtime tool result message for session {:?}", session_id);
    }
    Ok(())
}

fn call_ollama_generate(base_url: &str, model: &str, prompt: &str, images: Option<Vec<String>>) -> Result<String> {
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build Ollama HTTP client")?;
    let response = client
        .post(&url)
        .json(&OllamaGenerateRequest {
            model,
            prompt,
            stream: false,
            images,
        })
        .send()
        .with_context(|| format!("failed to call Ollama at {url}"))?
        .error_for_status()
        .with_context(|| format!("Ollama returned an error for {url}"))?;
    let payload: OllamaGenerateResponse = response.json().context("failed to decode Ollama response")?;
    Ok(payload.response.trim().to_string())
}

fn encode_image_as_base64(path: &str) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read image attachment {:?}", path))?;
    Ok(BASE64_STANDARD.encode(bytes))
}

fn validate_ollama_base_url(base_url: &str) -> Result<()> {
    if std::env::var("VELA_ALLOW_REMOTE_OLLAMA")
        .ok()
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
    {
        return Ok(());
    }

    let parsed = reqwest::Url::parse(base_url)
        .with_context(|| format!("invalid Ollama base URL {:?}", base_url))?;
    let host = parsed.host_str().context("Ollama base URL is missing a host")?;
    let is_local = host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().map(|ip| {
            ip.is_loopback() || ip == IpAddr::V4(Ipv4Addr::LOCALHOST) || ip == IpAddr::V6(Ipv6Addr::LOCALHOST)
        }).unwrap_or(false);

    if !is_local {
        bail!(
            "refusing non-local Ollama endpoint {:?}; set VELA_ALLOW_REMOTE_OLLAMA=1 to opt in explicitly",
            base_url
        );
    }
    Ok(())
}

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
    let parent = path.parent().ok_or_else(|| anyhow::anyhow!("scheduler jobs path has no parent directory"))?;
    let temp_path = parent.join(format!("{}.tmp-{}", path.file_name().and_then(|n| n.to_str()).unwrap_or("jobs.json"), unix_timestamp_nanos()));
    std::fs::write(&temp_path, serde_json::to_string_pretty(jobs)?)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

fn acquire_scheduler_jobs_lock(path: &std::path::Path) -> Result<SchedulerJobsLock> {
    let lock_path = path.with_extension("json.lock");
    for _ in 0..100 {
        match OpenOptions::new().write(true).create_new(true).open(&lock_path) {
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

fn validate_scheduler_job_id(id: &str) -> Result<&str> {
    let normalized = id.trim();
    if normalized.is_empty() || normalized == "." || normalized == ".." || normalized.contains('/') || normalized.contains('\\') {
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
    let Some(session) = vela_state::inspect_latest_session(&bootstrap.persistence.state_db_path, limit)? else {
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
        }
    }
}

pub use vela_memory::{MemoryTarget, MEMORY_CHAR_LIMIT, USER_CHAR_LIMIT};
pub use vela_state::SessionRequest;

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates an isolated bootstrap report for runtime regression tests.
    fn test_bootstrap(prefix: &str) -> BootstrapReport {
        let vela_home = std::env::temp_dir().join(format!("vela-runtime-{prefix}-{}", unix_timestamp_nanos()));
        BootstrapReport {
            vela_home: vela_home.clone(),
            active_profile: None,
            loaded_env_paths: vec![],
            ignored_user_config: false,
            config_sources: vec![],
            resolved_config: ResolvedConfig::default(),
            persistence: vela_state::initialize_persistence(&vela_home).unwrap(),
            memory: vela_memory::initialize_memory(&vela_home).unwrap(),
            skills: vela_skills::initialize_skills(&vela_home).unwrap(),
            reviews: vela_review::initialize_reviews(&vela_home).unwrap(),
        }
    }

    struct MockOllamaExchange<'a> {
        response_body: &'a str,
        expected_model: &'a str,
        prompt_fragment: &'a str,
        expected_image_base64: Option<&'a str>,
    }

    fn read_mock_http_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::Read;

        let mut request_bytes = Vec::new();
        let mut buf = [0u8; 4096];
        let header_end;
        let expected_total_len;
        loop {
            let read = stream.read(&mut buf).expect("read mock Ollama request");
            assert!(read > 0, "mock Ollama request closed before full payload arrived");
            request_bytes.extend_from_slice(&buf[..read]);
            if let Some(end) = request_bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let end = end + 4;
                let head = String::from_utf8_lossy(&request_bytes[..end]).into_owned();
                let content_length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: ").or_else(|| line.strip_prefix("content-length: ")))
                    .expect("Content-Length header")
                    .trim()
                    .parse::<usize>()
                    .expect("parse Content-Length");
                header_end = end;
                expected_total_len = header_end + content_length;
                break;
            }
        }
        while request_bytes.len() < expected_total_len {
            let read = stream.read(&mut buf).expect("read mock Ollama request body");
            assert!(read > 0, "mock Ollama request closed before body finished");
            request_bytes.extend_from_slice(&buf[..read]);
        }
        String::from_utf8_lossy(&request_bytes[..expected_total_len]).into_owned()
    }

    fn assert_mock_ollama_request(request: &str, exchange: &MockOllamaExchange<'_>) {
        let (head, body_text) = request.split_once("\r\n\r\n").expect("split HTTP request");
        let request_line = head.lines().next().expect("HTTP request line");
        assert!(request_line.starts_with("POST /api/generate HTTP/1.1"));
        let payload_json: serde_json::Value = serde_json::from_str(body_text).expect("decode request body");
        assert_eq!(payload_json.get("model").and_then(|v| v.as_str()), Some(exchange.expected_model));
        assert_eq!(payload_json.get("stream").and_then(|v| v.as_bool()), Some(false));
        let prompt = payload_json.get("prompt").and_then(|v| v.as_str()).expect("prompt field");
        assert!(prompt.contains(exchange.prompt_fragment));
        let images = payload_json.get("images").and_then(|v| v.as_array());
        if let Some(expected_image_base64) = exchange.expected_image_base64 {
            let images = images.expect("images field");
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].as_str(), Some(expected_image_base64));
        } else {
            assert!(payload_json.get("images").is_none(), "images field should be absent when no image is expected");
        }
    }

    fn spawn_mock_ollama_sequence(exchanges: Vec<MockOllamaExchange<'static>>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for exchange in exchanges {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_mock_http_request(&mut stream);
                assert_mock_ollama_request(&request, &exchange);
                let payload = serde_json::json!({ "response": exchange.response_body }).to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                );
                stream.write_all(reply.as_bytes()).unwrap();
                stream.flush().unwrap();
            }
        });
        (addr, handle)
    }

    fn spawn_mock_ollama(
        response_body: &'static str,
        expected_model: &'static str,
        prompt_fragment: &'static str,
        expected_image_base64: Option<&'static str>,
    ) -> (String, std::thread::JoinHandle<()>) {
        spawn_mock_ollama_sequence(vec![MockOllamaExchange {
            response_body,
            expected_model,
            prompt_fragment,
            expected_image_base64,
        }])
    }

    #[test]
    /// Verifies that scheduler registrations persist and duplicate pending jobs are rejected.
    fn scheduler_jobs_persist_and_dedupe() {
        let bootstrap = test_bootstrap("scheduler-test");

        let first = add_scheduled_job(&bootstrap, "0 * * * *", "ping status", None).unwrap();
        let jobs = list_scheduled_jobs(&bootstrap).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, first.id);

        let err = add_scheduled_job(&bootstrap, "0 * * * *", "ping status", None).unwrap_err();
        assert!(err.to_string().contains("already registered"));

        let fetched = get_scheduled_job(&bootstrap, &first.id).unwrap();
        assert_eq!(fetched.task, "ping status");

        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies gateway restart continuity without duplicating the bootstrap message.
    fn gateway_start_resumes_same_session_without_duplicate_bootstrap_message() {
        let bootstrap = test_bootstrap("gateway-resume");

        let first = start_gateway(&bootstrap).unwrap();
        let first_summary = current_command_session_summary(&bootstrap, "gateway")
            .unwrap()
            .expect("initial gateway session summary");
        let second = start_gateway(&bootstrap).unwrap();

        assert_eq!(first.session.session_id, second.session.session_id);
        let summary = current_command_session_summary(&bootstrap, "gateway")
            .unwrap()
            .expect("gateway session summary");
        assert_eq!(first_summary.message_count, 1);
        assert_eq!(summary.message_count, 1);
        assert_eq!(summary.event_count, first_summary.event_count + 2);

        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies scheduler restart continuity while preserving registered durable jobs.
    fn scheduler_start_resumes_same_session_and_preserves_registered_jobs() {
        let bootstrap = test_bootstrap("scheduler-resume");

        let first = start_scheduler(&bootstrap).unwrap();
        let first_summary = current_command_session_summary(&bootstrap, "cron")
            .unwrap()
            .expect("initial cron session summary");
        add_scheduled_job(&bootstrap, "*/5 * * * *", "ping status", Some("test")).unwrap();
        let second = start_scheduler(&bootstrap).unwrap();

        assert_eq!(first.session.session_id, second.session.session_id);
        assert_eq!(second.setup.job_count, 1);
        let summary = current_command_session_summary(&bootstrap, "cron")
            .unwrap()
            .expect("cron session summary");
        assert_eq!(first_summary.message_count, 1);
        assert_eq!(summary.message_count, 1);
        assert_eq!(summary.event_count, first_summary.event_count + 3);

        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that a query executes a live assistant turn and can emit review candidates.
    fn execute_chat_turn_appends_response_and_checkpoint_artifacts() {
        let bootstrap = test_bootstrap("chat-turn");
        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("please always use terse answers".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
            None,
            None,
            true,
        )
        .unwrap();

        assert!(report.response.as_deref().unwrap_or_default().contains("Vela executed a local kernel turn"));
        assert_eq!(report.emitted_signal_count, 1);
        assert_eq!(report.generated_candidate_count, 1);
        let summary = current_session_summary(&bootstrap).unwrap().expect("chat session summary");
        assert_eq!(summary.message_count, 2);
        assert!(list_review_candidates(&bootstrap).unwrap().len() >= 1);

        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that image-only turns still append an assistant response.
    fn execute_chat_turn_handles_image_only_requests() {
        let bootstrap = test_bootstrap("image-turn");
        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: false,
                query_text: None,
                image_present: true,
                image_path: Some("diagram.png".to_string()),
                resume: None,
                continue_last: None,
            },
            None,
            None,
            false,
        )
        .unwrap();

        assert!(report.response.as_deref().unwrap_or_default().contains("Vela executed a local image turn"));
        let summary = current_session_summary(&bootstrap).unwrap().expect("image chat session summary");
        assert_eq!(summary.message_count, 2);

        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that configured Ollama execution is used for image chat turns.
    fn execute_chat_turn_uses_ollama_provider_for_image_requests() {
        let (base_url, server) = spawn_mock_ollama(
            "Gemma inspected the image.",
            "gemma3:4b",
            "Please analyze the attached image",
            Some("ZmFrZS1wbmctYnl0ZXM="),
        );
        let mut bootstrap = test_bootstrap("ollama-image-turn");
        bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
        bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
        bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
        let image_path = bootstrap.vela_home.join("diagram.png");
        std::fs::create_dir_all(&bootstrap.vela_home).unwrap();
        std::fs::write(&image_path, b"fake-png-bytes").unwrap();

        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: false,
                query_text: None,
                image_present: true,
                image_path: Some(image_path.to_string_lossy().into_owned()),
                resume: None,
                continue_last: None,
            },
            None,
            None,
            false,
        )
        .unwrap();

        assert_eq!(report.response.as_deref(), Some("Gemma inspected the image."));
        assert_eq!(report.response_source, "runtime-ollama");
        let inspection = inspect_latest_session(&bootstrap, 10).unwrap().expect("image session inspection");
        let assistant = inspection.messages.last().expect("assistant message");
        let metadata: serde_json::Value = serde_json::from_str(
            assistant.metadata_json.as_deref().expect("assistant metadata"),
        )
        .expect("decode assistant metadata");
        assert_eq!(metadata.get("source").and_then(|v| v.as_str()), Some("runtime-ollama"));
        assert_eq!(metadata.get("provider").and_then(|v| v.as_str()), Some("ollama"));
        assert_eq!(metadata.get("model").and_then(|v| v.as_str()), Some("gemma3:4b"));
        server.join().unwrap();
        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that mixed text+image requests still forward the image payload to Ollama.
    fn execute_chat_turn_routes_mixed_text_and_image_requests_through_ollama_image_path() {
        let (base_url, server) = spawn_mock_ollama(
            "Gemma handled both prompt and image.",
            "gemma3:4b",
            "what is happening in this image?",
            Some("ZmFrZS1wbmctYnl0ZXM="),
        );
        let mut bootstrap = test_bootstrap("ollama-mixed-turn");
        bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
        bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
        bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
        let image_path = bootstrap.vela_home.join("diagram.png");
        std::fs::create_dir_all(&bootstrap.vela_home).unwrap();
        std::fs::write(&image_path, b"fake-png-bytes").unwrap();

        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("what is happening in this image?".to_string()),
                image_present: true,
                image_path: Some(image_path.to_string_lossy().into_owned()),
                resume: None,
                continue_last: None,
            },
            None,
            None,
            false,
        )
        .unwrap();

        assert_eq!(report.response.as_deref(), Some("Gemma handled both prompt and image."));
        assert_eq!(report.response_source, "runtime-ollama");
        server.join().unwrap();
        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that configured Ollama execution is used for text chat turns.
    fn execute_chat_turn_uses_ollama_provider_when_configured() {
        let (base_url, server) = spawn_mock_ollama("Gemma says hi.", "gemma3:4b", "hello there", None);
        let mut bootstrap = test_bootstrap("ollama-turn");
        bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
        bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
        bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);

        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("hello there".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
            None,
            None,
            false,
        )
        .unwrap();

        assert_eq!(report.response.as_deref(), Some("Gemma says hi."));
        assert_eq!(report.response_source, "runtime-ollama");
        server.join().unwrap();
        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that a configured provider turn can execute a bounded multi-step local tool sequence.
    fn execute_chat_turn_runs_first_runtime_tool_loop() {
        let (base_url, server) = spawn_mock_ollama_sequence(vec![
            MockOllamaExchange {
                response_body: r#"{"tool":"memory_snapshot"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "need the tool loop",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: r#"{"tool":"list_skills"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "Completed tool step 1 of 3.\nTool result for memory_snapshot:",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: "Tool-informed final answer.",
                expected_model: "gemma3:4b",
                prompt_fragment: "Completed tool step 2 of 3.\nTool result for list_skills:\ndeploy-staging",
                expected_image_base64: None,
            },
        ]);
        let mut bootstrap = test_bootstrap("ollama-tool-loop");
        bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
        bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
        bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
        std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
        std::fs::write(
            bootstrap.vela_home.join("skills").join("deploy-staging").join("SKILL.md"),
            "# deploy-staging\n\nDeploys staging.",
        )
        .unwrap();

        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("need the tool loop".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
            None,
            None,
            false,
        )
        .unwrap();

        assert_eq!(report.response.as_deref(), Some("Tool-informed final answer."));
        assert_eq!(report.response_source, "runtime-ollama-tool-loop");
        let inspection = inspect_latest_session(&bootstrap, 10).unwrap().expect("tool loop inspection");
        assert_eq!(inspection.messages.len(), 6);
        assert_eq!(inspection.messages[1].role, "tool-request");
        assert_eq!(inspection.messages[1].content, "memory_snapshot");
        assert_eq!(inspection.messages[2].role, "tool-result");
        assert!(!inspection.messages[2].content.trim().is_empty());
        let first_tool_result_metadata: serde_json::Value = serde_json::from_str(
            inspection.messages[2].metadata_json.as_deref().expect("first tool-result metadata"),
        )
        .expect("decode first tool-result metadata");
        assert_eq!(first_tool_result_metadata.get("tool").and_then(|v| v.as_str()), Some("memory_snapshot"));
        assert_eq!(first_tool_result_metadata.get("step").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(inspection.messages[3].role, "tool-request");
        assert_eq!(inspection.messages[3].content, "list_skills");
        assert_eq!(inspection.messages[4].role, "tool-result");
        assert!(inspection.messages[4].content.contains("deploy-staging"));
        assert_eq!(inspection.events.iter().filter(|event| event.event_type == "runtime_tool_requested").count(), 2);
        assert_eq!(inspection.events.iter().filter(|event| event.event_type == "runtime_tool_completed").count(), 2);
        let assistant = inspection.messages.last().expect("assistant message");
        let metadata: serde_json::Value = serde_json::from_str(
            assistant.metadata_json.as_deref().expect("assistant metadata"),
        )
        .expect("decode assistant metadata");
        assert_eq!(metadata.get("source").and_then(|v| v.as_str()), Some("runtime-ollama-tool-loop"));
        server.join().unwrap();
        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that the iterative tool loop trips the max-step guard and falls back deterministically.
    fn execute_chat_turn_stops_at_max_runtime_tool_steps() {
        let (base_url, server) = spawn_mock_ollama_sequence(vec![
            MockOllamaExchange {
                response_body: r#"{"tool":"memory_snapshot"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "trip the max-step guard",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: r#"{"tool":"list_skills"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "Completed tool step 1 of 3.\nTool result for memory_snapshot:",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: r#"{"tool":"memory_snapshot"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "Completed tool step 2 of 3.\nTool result for list_skills:\ndeploy-staging",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: r#"{"tool":"list_skills"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "Completed tool step 3 of 3.\nTool result for memory_snapshot:",
                expected_image_base64: None,
            },
        ]);
        let mut bootstrap = test_bootstrap("ollama-tool-loop-max-step");
        bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
        bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
        bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
        std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
        std::fs::write(
            bootstrap.vela_home.join("skills").join("deploy-staging").join("SKILL.md"),
            "# deploy-staging\n\nDeploys staging.",
        )
        .unwrap();

        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("trip the max-step guard".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
            None,
            None,
            false,
        )
        .unwrap();

        assert_eq!(report.response_source, "runtime-kernel");
        assert!(report
            .response
            .as_deref()
            .unwrap_or_default()
            .contains("maximum bounded tool steps"));
        let inspection = inspect_latest_session(&bootstrap, 12).unwrap().expect("max-step inspection");
        assert_eq!(inspection.messages.len(), 8);
        assert_eq!(inspection.events.iter().filter(|event| event.event_type == "runtime_tool_requested").count(), 3);
        assert_eq!(inspection.events.iter().filter(|event| event.event_type == "runtime_tool_completed").count(), 3);
        let third_tool_result_metadata: serde_json::Value = serde_json::from_str(
            inspection.messages[6].metadata_json.as_deref().expect("third tool-result metadata"),
        )
        .expect("decode third tool-result metadata");
        assert_eq!(inspection.messages[6].role, "tool-result");
        assert_eq!(third_tool_result_metadata.get("tool").and_then(|v| v.as_str()), Some("memory_snapshot"));
        assert_eq!(third_tool_result_metadata.get("step").and_then(|v| v.as_u64()), Some(3));
        let assistant = inspection.messages.last().expect("assistant message");
        let assistant_metadata: serde_json::Value = serde_json::from_str(
            assistant.metadata_json.as_deref().expect("assistant metadata"),
        )
        .expect("decode assistant metadata");
        assert_eq!(assistant_metadata.get("source").and_then(|v| v.as_str()), Some("runtime-kernel"));
        server.join().unwrap();
        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }
}
