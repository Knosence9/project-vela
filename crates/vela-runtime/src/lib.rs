use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
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
pub struct GatewaySetupReport {
    pub gateway_dir: std::path::PathBuf,
    pub config_path: std::path::PathBuf,
    pub inbox_dir: std::path::PathBuf,
    pub outbox_dir: std::path::PathBuf,
    pub config_existed_before: bool,
}

#[derive(Debug, Clone)]
pub struct GatewayStartReport {
    pub setup: GatewaySetupReport,
    pub session: SessionRuntimeReport,
}

#[derive(Debug, Clone)]
pub struct SchedulerSetupReport {
    pub scheduler_dir: std::path::PathBuf,
    pub config_path: std::path::PathBuf,
    pub jobs_path: std::path::PathBuf,
    pub config_existed_before: bool,
    pub jobs_existed_before: bool,
    pub job_count: usize,
}

#[derive(Debug, Clone)]
pub struct SchedulerStartReport {
    pub setup: SchedulerSetupReport,
    pub session: SessionRuntimeReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: String,
    pub schedule: String,
    pub task: String,
    pub source: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
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

pub fn initialize_bootstrap(active_profile: Option<String>, ignore_user_config: bool) -> Result<BootstrapReport> {
    let config = vela_config::initialize_config(active_profile, ignore_user_config)?;
    let persistence = vela_state::initialize_persistence(&config.vela_home)?;
    let memory = vela_memory::initialize_memory(&config.vela_home)?;
    let skills = vela_skills::initialize_skills(&config.vela_home)?;
    let reviews = vela_review::initialize_reviews(&config.vela_home)?;
    Ok(BootstrapReport::from_parts(config, persistence, memory, skills, reviews))
}

pub fn bootstrap_banner() {
    tracing::debug!("vela-runtime bootstrap initialized");
}

pub fn current_session_identity(bootstrap: &BootstrapReport) -> Result<Option<(String, String)>> {
    vela_state::current_session_identity(&bootstrap.persistence.state_db_path)
}

pub fn current_session_summary(bootstrap: &BootstrapReport) -> Result<Option<SessionSummary>> {
    vela_state::current_session_summary(&bootstrap.persistence.state_db_path)
}

pub fn current_command_session_summary(
    bootstrap: &BootstrapReport,
    command_name: &str,
) -> Result<Option<SessionSummary>> {
    vela_state::current_command_session_summary(&bootstrap.persistence.state_db_path, command_name)
}

pub fn resolve_runtime_session(bootstrap: &BootstrapReport, request: &SessionRequest) -> Result<SessionRuntimeReport> {
    vela_state::resolve_runtime_session(&bootstrap.persistence.state_db_path, request)
}

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

pub fn list_scheduled_jobs(bootstrap: &BootstrapReport) -> Result<Vec<ScheduledJob>> {
    let setup = setup_scheduler(bootstrap)?;
    load_scheduler_jobs(&setup.jobs_path)
}

pub fn get_scheduled_job(bootstrap: &BootstrapReport, id: &str) -> Result<ScheduledJob> {
    let job_id = validate_scheduler_job_id(id)?;
    list_scheduled_jobs(bootstrap)?
        .into_iter()
        .find(|job| job.id == job_id)
        .ok_or_else(|| anyhow::anyhow!("scheduled job {:?} not found", job_id))
}

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
    let mut jobs = load_scheduler_jobs(&setup.jobs_path)?;
    if jobs.iter().any(|job| job.schedule == schedule && job.task == task && job.source == source && job.status == "pending") {
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

pub fn search_session_history(bootstrap: &BootstrapReport, query: &str, limit: usize) -> Result<Vec<SessionSearchHit>> {
    vela_state::search_session_history(&bootstrap.persistence.state_db_path, query, limit)
}

pub fn inspect_latest_session(bootstrap: &BootstrapReport, limit: usize) -> Result<Option<SessionInspection>> {
    vela_state::inspect_latest_session(&bootstrap.persistence.state_db_path, limit)
}

pub fn render_memory_snapshot(bootstrap: &BootstrapReport) -> Result<String> {
    vela_memory::render_prompt_snapshot(&bootstrap.vela_home)
}

pub fn view_memory(bootstrap: &BootstrapReport, target: vela_memory::MemoryTarget) -> Result<vela_memory::MemoryView> {
    vela_memory::view_memory(&bootstrap.vela_home, target)
}

pub fn add_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    content: &str,
) -> Result<vela_memory::MemoryMutationReport> {
    vela_memory::add_memory_entry(&bootstrap.vela_home, target, content)
}

pub fn stage_add_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    content: &str,
) -> Result<vela_memory::PendingMemoryWrite> {
    vela_memory::stage_add_memory_entry(&bootstrap.vela_home, target, content)
}

pub fn replace_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    old_text: &str,
    content: &str,
) -> Result<vela_memory::MemoryMutationReport> {
    vela_memory::replace_memory_entry(&bootstrap.vela_home, target, old_text, content)
}

pub fn stage_replace_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    old_text: &str,
    content: &str,
) -> Result<vela_memory::PendingMemoryWrite> {
    vela_memory::stage_replace_memory_entry(&bootstrap.vela_home, target, old_text, content)
}

pub fn remove_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    old_text: &str,
) -> Result<vela_memory::MemoryMutationReport> {
    vela_memory::remove_memory_entry(&bootstrap.vela_home, target, old_text)
}

pub fn stage_remove_memory_entry(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
    old_text: &str,
) -> Result<vela_memory::PendingMemoryWrite> {
    vela_memory::stage_remove_memory_entry(&bootstrap.vela_home, target, old_text)
}

pub fn list_pending_memory(bootstrap: &BootstrapReport) -> Result<Vec<vela_memory::PendingMemoryWrite>> {
    vela_memory::list_pending(&bootstrap.vela_home)
}

pub fn get_pending_memory(bootstrap: &BootstrapReport, id: &str) -> Result<vela_memory::PendingMemoryWrite> {
    vela_memory::get_pending(&bootstrap.vela_home, id)
}

pub fn approve_pending_memory(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_memory::MemoryMutationReport> {
    vela_memory::approve_pending(&bootstrap.vela_home, id)
}

pub fn reject_pending_memory(bootstrap: &BootstrapReport, id: &str) -> Result<()> {
    vela_memory::reject_pending(&bootstrap.vela_home, id)
}

pub fn list_skills(bootstrap: &BootstrapReport) -> Result<Vec<vela_skills::SkillSummary>> {
    vela_skills::list_skills(&bootstrap.vela_home)
}

pub fn view_skill(bootstrap: &BootstrapReport, name: &str) -> Result<vela_skills::SkillView> {
    vela_skills::view_skill(&bootstrap.vela_home, name)
}

pub fn create_skill(
    bootstrap: &BootstrapReport,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<vela_skills::SkillMutationReport> {
    vela_skills::create_skill(&bootstrap.vela_home, name, description, body)
}

pub fn stage_create_skill(
    bootstrap: &BootstrapReport,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<vela_skills::PendingSkillWrite> {
    vela_skills::stage_create_skill(&bootstrap.vela_home, name, description, body)
}

pub fn write_skill(
    bootstrap: &BootstrapReport,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<vela_skills::SkillMutationReport> {
    vela_skills::write_skill(&bootstrap.vela_home, name, description, body)
}

pub fn stage_write_skill(
    bootstrap: &BootstrapReport,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<vela_skills::PendingSkillWrite> {
    vela_skills::stage_write_skill(&bootstrap.vela_home, name, description, body)
}

pub fn delete_skill(bootstrap: &BootstrapReport, name: &str) -> Result<vela_skills::SkillMutationReport> {
    vela_skills::delete_skill(&bootstrap.vela_home, name)
}

pub fn stage_delete_skill(bootstrap: &BootstrapReport, name: &str) -> Result<vela_skills::PendingSkillWrite> {
    vela_skills::stage_delete_skill(&bootstrap.vela_home, name)
}

pub fn list_pending_skills(bootstrap: &BootstrapReport) -> Result<Vec<vela_skills::PendingSkillWrite>> {
    vela_skills::list_pending(&bootstrap.vela_home)
}

pub fn get_pending_skill(bootstrap: &BootstrapReport, id: &str) -> Result<vela_skills::PendingSkillWrite> {
    vela_skills::get_pending(&bootstrap.vela_home, id)
}

pub fn approve_pending_skill(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_skills::SkillMutationReport> {
    vela_skills::approve_pending(&bootstrap.vela_home, id)
}

pub fn reject_pending_skill(bootstrap: &BootstrapReport, id: &str) -> Result<()> {
    vela_skills::reject_pending(&bootstrap.vela_home, id)
}

pub fn list_review_candidates(bootstrap: &BootstrapReport) -> Result<Vec<vela_review::ReviewCandidate>> {
    vela_review::list_candidates(&bootstrap.vela_home)
}

pub fn get_review_candidate(bootstrap: &BootstrapReport, id: &str) -> Result<vela_review::ReviewCandidate> {
    vela_review::get_candidate(&bootstrap.vela_home, id)
}

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
    std::fs::write(path, serde_json::to_string_pretty(jobs)?)?;
    Ok(())
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

    #[test]
    fn scheduler_jobs_persist_and_dedupe() {
        let vela_home = std::env::temp_dir().join(format!("vela-runtime-scheduler-test-{}", unix_timestamp_nanos()));
        let bootstrap = BootstrapReport {
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
        };

        let first = add_scheduled_job(&bootstrap, "0 * * * *", "ping status", None).unwrap();
        let jobs = list_scheduled_jobs(&bootstrap).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, first.id);

        let err = add_scheduled_job(&bootstrap, "0 * * * *", "ping status", None).unwrap_err();
        assert!(err.to_string().contains("already registered"));

        let fetched = get_scheduled_job(&bootstrap, &first.id).unwrap();
        assert_eq!(fetched.task, "ping status");

        std::fs::remove_dir_all(&vela_home).unwrap();
    }
}
