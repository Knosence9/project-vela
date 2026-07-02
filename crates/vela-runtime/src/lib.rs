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
    pub executed_job_count: usize,
    pub recovered_job_count: usize,
    pub failed_job_count: usize,
}

#[derive(Debug, Clone)]
/// Captures one executed chat/runtime turn plus any operator-triggered review work.
pub struct ChatTurnReport {
    pub session: SessionRuntimeReport,
    pub turn_id: String,
    pub response: Option<String>,
    pub response_source: String,
    pub lifecycle_phase_count: usize,
    pub final_phase: String,
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
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub next_run_at: i64,
    #[serde(default)]
    pub last_started_at: Option<i64>,
    #[serde(default)]
    pub last_completed_at: Option<i64>,
    #[serde(default)]
    pub last_failed_at: Option<i64>,
    #[serde(default)]
    pub last_recovered_at: Option<i64>,
    #[serde(default)]
    pub last_outcome: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub run_count: u64,
    #[serde(default)]
    pub recovery_count: u64,
    #[serde(default)]
    pub last_session_id: Option<String>,
    #[serde(default)]
    pub execution_token: Option<String>,
    #[serde(default)]
    pub lease_expires_at: Option<i64>,
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
    pub extensions: ExtensionsReport,
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
                    vela_config::ConfigSourceKind::User
                        | vela_config::ConfigSourceKind::ProjectFallback
                )
            })
            .count();
        format!(
            "vela bootstrap ready: home={} env_files={} config_files={} ignore_user_config={} state_db_runs={} extensions_activated={} extensions_validated={} extensions_disabled={} extensions_failed={}{}",
            self.vela_home.display(),
            env_count,
            config_count,
            self.ignored_user_config,
            self.persistence.bootstrap_runs,
            self.extensions.activated_count,
            self.extensions.validated_count,
            self.extensions.disabled_count,
            self.extensions.failed_count,
            profile
        )
    }
}

/// Initializes config, persistence, memory, skills, review, and extension-registry subsystems.
pub fn initialize_bootstrap(
    active_profile: Option<String>,
    ignore_user_config: bool,
) -> Result<BootstrapReport> {
    let config = vela_config::initialize_config(active_profile, ignore_user_config)?;
    let persistence = vela_state::initialize_persistence(&config.vela_home)?;
    let memory = vela_memory::initialize_memory(&config.vela_home)?;
    let skills = vela_skills::initialize_skills(&config.vela_home)?;
    let reviews = vela_review::initialize_reviews(&config.vela_home)?;
    let extensions =
        vela_extensions::initialize_extensions(&config.vela_home, &config.resolved_config)?;
    Ok(BootstrapReport::from_parts(
        config,
        persistence,
        memory,
        skills,
        reviews,
        extensions,
    ))
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

/// Reloads extension discovery from the latest config and manifest files without resetting durable session state.
pub fn reload_extensions(bootstrap: &BootstrapReport) -> Result<ExtensionsReport> {
    let (_, resolved_config) =
        vela_config::reload_config_snapshot(&bootstrap.vela_home, bootstrap.ignored_user_config)?;
    vela_extensions::initialize_extensions(&bootstrap.vela_home, &resolved_config)
}

/// Resolves or creates a runtime session for an interactive request.
pub fn resolve_runtime_session(
    bootstrap: &BootstrapReport,
    request: &SessionRequest,
) -> Result<SessionRuntimeReport> {
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
    let cycle = process_scheduler_jobs(bootstrap, &session.session_id, &setup.jobs_path)?;
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
            "executed_job_count": cycle.executed_job_count,
            "recovered_job_count": cycle.recovered_job_count,
            "failed_job_count": cycle.failed_job_count,
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
    Ok(SchedulerStartReport {
        setup,
        session,
        executed_job_count: cycle.executed_job_count,
        recovered_job_count: cycle.recovered_job_count,
        failed_job_count: cycle.failed_job_count,
    })
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
    let mut lifecycle = RuntimeTurnRecorder::new();
    lifecycle.record_phase(
        bootstrap,
        &session.session_id,
        "receive",
        None,
        json!({
            "action": session.action.label(),
            "interaction_mode": session.interaction_mode.label(),
            "query_present": request.query_present,
            "image_present": request.image_present,
            "resume": request.resume,
            "continue_last": request.continue_last,
        }),
    )?;
    lifecycle.record_phase(
        bootstrap,
        &session.session_id,
        "deliberate",
        None,
        json!({
            "provider_override": provider_override,
            "model_override": model_override,
            "checkpoints": checkpoints,
        }),
    )?;
    let rendered = match render_chat_response(
        bootstrap,
        &session,
        request,
        provider_override,
        model_override,
        &mut lifecycle,
    ) {
        Ok(rendered) => rendered,
        Err(error) => {
            let _ = lifecycle.record_phase(
                bootstrap,
                &session.session_id,
                "failed",
                None,
                json!({"error": error.to_string()}),
            );
            return Err(error);
        }
    };

    let post_render = (|| -> Result<ChatTurnReport> {
        let mut assistant_persisted = false;
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
                        "turn_id": lifecycle.turn_id,
                    })
                    .to_string(),
                ),
            )?;
            assistant_persisted = logged;
            if !logged {
                tracing::warn!(session_id=%session.session_id, "failed to append assistant runtime response");
            }
        }
        lifecycle.record_phase(
            bootstrap,
            &session.session_id,
            "respond",
            None,
            json!({
                "source": rendered.source,
                "provider": rendered.provider,
                "model": rendered.model,
                "content_present": rendered.content.is_some(),
                "assistant_persisted": assistant_persisted,
            }),
        )?;

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
        lifecycle.record_phase(
            bootstrap,
            &session.session_id,
            "finish",
            None,
            json!({
                "response_source": rendered.source,
                "emitted_signal_count": emitted_signal_count,
                "generated_candidate_count": generated_candidate_count,
            }),
        )?;

        Ok(ChatTurnReport {
            session: session.clone(),
            turn_id: lifecycle.turn_id.clone(),
            response: rendered.content,
            response_source: rendered.source.to_string(),
            lifecycle_phase_count: lifecycle.phase_count(),
            final_phase: lifecycle.final_phase().to_string(),
            emitted_signal_count,
            generated_candidate_count,
        })
    })();

    match post_render {
        Ok(report) => Ok(report),
        Err(error) => {
            let _ = lifecycle.record_phase(
                bootstrap,
                &session.session_id,
                "failed",
                None,
                json!({"error": error.to_string()}),
            );
            Err(error)
        }
    }
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
    if jobs.iter().any(|job| {
        job.schedule == schedule
            && job.task == task
            && job.source == source
            && matches!(job.status.as_str(), "pending" | "running")
    }) {
        drop(lock);
        bail!("matching scheduled job is already registered");
    }
    let now = unix_timestamp();
    let job = ScheduledJob {
        id: format!("job-{}", unix_timestamp_nanos()),
        next_run_at: next_scheduler_run_at(&schedule, now),
        schedule,
        task,
        source,
        status: "pending".to_string(),
        created_at: now,
        updated_at: now,
        last_started_at: None,
        last_completed_at: None,
        last_failed_at: None,
        last_recovered_at: None,
        last_outcome: None,
        last_error: None,
        run_count: 0,
        recovery_count: 0,
        last_session_id: None,
        execution_token: None,
        lease_expires_at: None,
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

#[derive(Default)]
struct SchedulerCycleReport {
    executed_job_count: usize,
    recovered_job_count: usize,
    failed_job_count: usize,
}

const SCHEDULER_RECOVERY_LEASE_SECONDS: i64 = 300;

fn process_scheduler_jobs(
    bootstrap: &BootstrapReport,
    scheduler_session_id: &str,
    jobs_path: &std::path::Path,
) -> Result<SchedulerCycleReport> {
    let now = unix_timestamp();
    let lock = acquire_scheduler_jobs_lock(jobs_path)?;
    let mut jobs = load_scheduler_jobs(jobs_path)?;
    let mut recovered_job_ids = Vec::new();
    for job in &mut jobs {
        backfill_scheduler_job(job, now);
        if job.status == "running"
            && job.lease_expires_at.is_some_and(|lease| lease <= now)
            && job.execution_token.is_some()
        {
            job.status = "pending".to_string();
            job.updated_at = now;
            job.last_recovered_at = Some(now);
            job.last_outcome = Some("recovered".to_string());
            job.recovery_count += 1;
            job.execution_token = None;
            job.lease_expires_at = None;
            recovered_job_ids.push(job.id.clone());
        }
    }
    let due_job_ids = jobs
        .iter()
        .filter(|job| matches!(job.status.as_str(), "pending" | "failed") && job.next_run_at <= now)
        .map(|job| job.id.clone())
        .collect::<Vec<_>>();
    save_scheduler_jobs(jobs_path, &jobs)?;
    drop(lock);

    for job_id in &recovered_job_ids {
        let event_logged = vela_state::append_event_to_session(
            &bootstrap.persistence.state_db_path,
            scheduler_session_id,
            "scheduler_job_recovered",
            json!({"job_id": job_id, "recovered_at": now}).to_string(),
        )?;
        if !event_logged {
            tracing::warn!(session_id=%scheduler_session_id, job_id=%job_id, "failed to append scheduler recovery event");
        }
    }

    let mut cycle = SchedulerCycleReport {
        recovered_job_count: recovered_job_ids.len(),
        ..SchedulerCycleReport::default()
    };
    for job_id in due_job_ids {
        match execute_scheduled_job(bootstrap, scheduler_session_id, jobs_path, &job_id) {
            Ok(true) => cycle.executed_job_count += 1,
            Ok(false) => {}
            Err(error) => {
                cycle.failed_job_count += 1;
                tracing::warn!(job_id=%job_id, error=%error, "scheduled job execution failed");
            }
        }
    }
    Ok(cycle)
}

fn execute_scheduled_job(
    bootstrap: &BootstrapReport,
    scheduler_session_id: &str,
    jobs_path: &std::path::Path,
    job_id: &str,
) -> Result<bool> {
    let now = unix_timestamp();
    let lock = acquire_scheduler_jobs_lock(jobs_path)?;
    let mut jobs = load_scheduler_jobs(jobs_path)?;
    let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) else {
        drop(lock);
        return Ok(false);
    };
    backfill_scheduler_job(job, now);
    if !matches!(job.status.as_str(), "pending" | "failed") || job.next_run_at > now {
        drop(lock);
        return Ok(false);
    }
    let execution_token = format!("attempt-{}", unix_timestamp_nanos());
    job.status = "running".to_string();
    job.updated_at = now;
    job.last_started_at = Some(now);
    job.last_outcome = Some("running".to_string());
    job.last_error = None;
    job.execution_token = Some(execution_token.clone());
    job.lease_expires_at = Some(now + SCHEDULER_RECOVERY_LEASE_SECONDS);
    let task = job.task.clone();
    let schedule = job.schedule.clone();
    save_scheduler_jobs(jobs_path, &jobs)?;
    drop(lock);

    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        scheduler_session_id,
        "scheduler_job_started",
        json!({"job_id": job_id, "task": task, "schedule": schedule, "started_at": now, "execution_token": execution_token})
            .to_string(),
    )?;
    if !event_logged {
        tracing::warn!(session_id=%scheduler_session_id, job_id=%job_id, "failed to append scheduler job start event");
    }

    match execute_chat_turn(
        bootstrap,
        &SessionRequest {
            command_name: "cron".to_string(),
            query_present: true,
            query_text: Some(task.clone()),
            image_present: false,
            image_path: None,
            resume: Some(scheduler_session_id.to_string()),
            continue_last: None,
        },
        None,
        None,
        false,
    ) {
        Ok(report) => {
            let completed_at = unix_timestamp();
            let lock = acquire_scheduler_jobs_lock(jobs_path)?;
            let mut jobs = load_scheduler_jobs(jobs_path)?;
            if let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) {
                backfill_scheduler_job(job, completed_at);
                if job.execution_token.as_deref() != Some(execution_token.as_str()) {
                    drop(lock);
                    return Ok(false);
                }
                job.status = "pending".to_string();
                job.updated_at = completed_at;
                job.last_completed_at = Some(completed_at);
                job.last_outcome = Some("completed".to_string());
                job.last_error = None;
                job.run_count += 1;
                job.last_session_id = Some(report.session.session_id.clone());
                job.next_run_at = next_scheduler_run_at(&job.schedule, completed_at);
                job.execution_token = None;
                job.lease_expires_at = None;
            }
            save_scheduler_jobs(jobs_path, &jobs)?;
            drop(lock);

            let event_logged = vela_state::append_event_to_session(
                &bootstrap.persistence.state_db_path,
                scheduler_session_id,
                "scheduler_job_completed",
                json!({
                    "job_id": job_id,
                    "completed_at": completed_at,
                    "response_source": report.response_source,
                    "session_id": report.session.session_id,
                    "execution_token": execution_token,
                })
                .to_string(),
            )?;
            if !event_logged {
                tracing::warn!(session_id=%scheduler_session_id, job_id=%job_id, "failed to append scheduler job completion event");
            }
            Ok(true)
        }
        Err(error) => {
            let failed_at = unix_timestamp();
            let lock = acquire_scheduler_jobs_lock(jobs_path)?;
            let mut jobs = load_scheduler_jobs(jobs_path)?;
            if let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) {
                backfill_scheduler_job(job, failed_at);
                if job.execution_token.as_deref() != Some(execution_token.as_str()) {
                    drop(lock);
                    return Ok(false);
                }
                job.status = "failed".to_string();
                job.updated_at = failed_at;
                job.last_failed_at = Some(failed_at);
                job.last_outcome = Some("failed".to_string());
                job.last_error = Some(error.to_string());
                job.next_run_at = next_scheduler_run_at(&job.schedule, failed_at);
                job.execution_token = None;
                job.lease_expires_at = None;
            }
            save_scheduler_jobs(jobs_path, &jobs)?;
            drop(lock);

            let event_logged = vela_state::append_event_to_session(
                &bootstrap.persistence.state_db_path,
                scheduler_session_id,
                "scheduler_job_failed",
                json!({"job_id": job_id, "failed_at": failed_at, "error": error.to_string(), "execution_token": execution_token})
                    .to_string(),
            )?;
            if !event_logged {
                tracing::warn!(session_id=%scheduler_session_id, job_id=%job_id, "failed to append scheduler job failure event");
            }
            Err(error)
        }
    }
}

fn backfill_scheduler_job(job: &mut ScheduledJob, now: i64) {
    if job.updated_at == 0 {
        job.updated_at = job.created_at.max(now);
    }
    if job.next_run_at == 0 {
        job.next_run_at = next_scheduler_run_at(&job.schedule, job.created_at.max(now));
    }
}

fn next_scheduler_run_at(schedule: &str, now: i64) -> i64 {
    let fields = schedule.split_whitespace().collect::<Vec<_>>();
    match fields.as_slice() {
        ["*", "*", "*", "*", "*"] => now - (now % 60) + 60,
        [minute, "*", "*", "*", "*"] if minute.starts_with("*/") => minute
            .trim_start_matches("*/")
            .parse::<i64>()
            .ok()
            .filter(|value| *value > 0)
            .map(|value| now - (now % (value * 60)) + (value * 60))
            .unwrap_or(now + 60),
        ["0", "*", "*", "*", "*"] => now - (now % 3600) + 3600,
        ["0", "0", "*", "*", "*"] => now - (now % 86_400) + 86_400,
        _ => now + 60,
    }
}

/// Searches persisted session history using the state FTS index.
pub fn search_session_history(
    bootstrap: &BootstrapReport,
    query: &str,
    limit: usize,
) -> Result<Vec<SessionSearchHit>> {
    vela_state::search_session_history(&bootstrap.persistence.state_db_path, query, limit)
}

/// Inspects the latest persisted session with recent messages and events.
pub fn inspect_latest_session(
    bootstrap: &BootstrapReport,
    limit: usize,
) -> Result<Option<SessionInspection>> {
    vela_state::inspect_latest_session(&bootstrap.persistence.state_db_path, limit)
}

/// Inspects one persisted session by id or title.
pub fn inspect_session(
    bootstrap: &BootstrapReport,
    target: &str,
    limit: usize,
) -> Result<Option<SessionInspection>> {
    vela_state::inspect_session(&bootstrap.persistence.state_db_path, target, limit)
}

/// Creates a durable branch session with copied continuity and explicit lineage.
pub fn branch_session(
    bootstrap: &BootstrapReport,
    source: &str,
    title: Option<&str>,
    note: Option<&str>,
) -> Result<SessionBranchRecord> {
    vela_state::branch_session(&bootstrap.persistence.state_db_path, source, title, note)
}

/// Persists one compression summary for a session.
pub fn compress_session(
    bootstrap: &BootstrapReport,
    target: &str,
    summary: &str,
) -> Result<SessionCompressionRecord> {
    vela_state::compress_session(&bootstrap.persistence.state_db_path, target, summary)
}

/// Renders the always-on memory snapshot used for prompting.
pub fn render_memory_snapshot(bootstrap: &BootstrapReport) -> Result<String> {
    vela_memory::render_prompt_snapshot(&bootstrap.vela_home)
}

/// Views the current durable memory contents for a target file.
pub fn view_memory(
    bootstrap: &BootstrapReport,
    target: vela_memory::MemoryTarget,
) -> Result<vela_memory::MemoryView> {
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
pub fn list_pending_memory(
    bootstrap: &BootstrapReport,
) -> Result<Vec<vela_memory::PendingMemoryWrite>> {
    vela_memory::list_pending(&bootstrap.vela_home)
}

/// Loads one staged memory write by id.
pub fn get_pending_memory(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_memory::PendingMemoryWrite> {
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
pub fn delete_skill(
    bootstrap: &BootstrapReport,
    name: &str,
) -> Result<vela_skills::SkillMutationReport> {
    vela_skills::delete_skill(&bootstrap.vela_home, name)
}

/// Stages deletion of a durable skill for later approval.
pub fn stage_delete_skill(
    bootstrap: &BootstrapReport,
    name: &str,
) -> Result<vela_skills::PendingSkillWrite> {
    vela_skills::stage_delete_skill(&bootstrap.vela_home, name)
}

/// Lists staged skill writes awaiting approval.
pub fn list_pending_skills(
    bootstrap: &BootstrapReport,
) -> Result<Vec<vela_skills::PendingSkillWrite>> {
    vela_skills::list_pending(&bootstrap.vela_home)
}

/// Loads one staged skill write by id.
pub fn get_pending_skill(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_skills::PendingSkillWrite> {
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
pub fn list_review_candidates(
    bootstrap: &BootstrapReport,
) -> Result<Vec<vela_review::ReviewCandidate>> {
    vela_review::list_candidates(&bootstrap.vela_home)
}

/// Loads one review candidate by id.
pub fn get_review_candidate(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_review::ReviewCandidate> {
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
pub fn promote_review_candidate(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_review::PromotionReport> {
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

struct RenderedChatResponse {
    content: Option<String>,
    source: &'static str,
    provider: Option<String>,
    model: Option<String>,
}

trait RuntimeProviderBackend {
    fn label(&self) -> &str;
    fn model(&self) -> Option<&str>;
    fn validate(&self) -> Result<()>;
    fn generate(&self, prompt: &str, images: Option<Vec<String>>) -> Result<String>;
    fn direct_response_source(&self) -> &'static str;
    fn tool_loop_response_source(&self) -> &'static str;
}

#[derive(Debug, Clone)]
struct OllamaRuntimeProvider {
    label: String,
    model: Option<String>,
    base_url: String,
}

impl RuntimeProviderBackend for OllamaRuntimeProvider {
    fn label(&self) -> &str {
        &self.label
    }

    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn validate(&self) -> Result<()> {
        validate_ollama_base_url(&self.base_url)
    }

    fn generate(&self, prompt: &str, images: Option<Vec<String>>) -> Result<String> {
        let model = self.model.as_deref().context(
            "runtime provider 'ollama' requires a model (for example a Gemma family model)",
        )?;
        call_ollama_generate(&self.base_url, model, prompt, images)
    }

    fn direct_response_source(&self) -> &'static str {
        "runtime-ollama"
    }

    fn tool_loop_response_source(&self) -> &'static str {
        "runtime-ollama-tool-loop"
    }
}

#[derive(Debug, Clone, Copy)]
enum RuntimeToolName {
    MemorySnapshot,
    ListSkills,
    ViewMemory,
    SearchSessionHistory,
    ViewSkill,
}

impl RuntimeToolName {
    fn as_str(self) -> &'static str {
        match self {
            Self::MemorySnapshot => "memory_snapshot",
            Self::ListSkills => "list_skills",
            Self::ViewMemory => "view_memory",
            Self::SearchSessionHistory => "search_session_history",
            Self::ViewSkill => "view_skill",
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeToolInvocation {
    name: RuntimeToolName,
    target: Option<vela_memory::MemoryTarget>,
    query: Option<String>,
    skill_name: Option<String>,
    limit: Option<usize>,
}

impl RuntimeToolInvocation {
    fn display_name(&self) -> &'static str {
        self.name.as_str()
    }

    fn request_text(&self) -> String {
        match self.name {
            RuntimeToolName::MemorySnapshot | RuntimeToolName::ListSkills => {
                self.display_name().to_string()
            }
            RuntimeToolName::ViewMemory => format!(
                "{}:{}",
                self.display_name(),
                self.target
                    .unwrap_or(vela_memory::MemoryTarget::Memory)
                    .label()
            ),
            RuntimeToolName::SearchSessionHistory => format!(
                "{}:{}",
                self.display_name(),
                self.query.as_deref().unwrap_or_default()
            ),
            RuntimeToolName::ViewSkill => format!(
                "{}:{}",
                self.display_name(),
                self.skill_name.as_deref().unwrap_or_default()
            ),
        }
    }

    fn metadata_json(&self) -> serde_json::Value {
        json!({
            "tool": self.display_name(),
            "target": self.target.map(|target| target.label().to_string()),
            "query": self.query,
            "skill_name": self.skill_name,
            "limit": self.limit,
        })
    }
}

const MAX_RUNTIME_TOOL_STEPS: usize = 3;
const MAX_RUNTIME_REFLECTION_ATTEMPTS: usize = 2;

struct RuntimeExecutionConfig {
    provider: Option<Box<dyn RuntimeProviderBackend>>,
    provider_label: Option<String>,
    model: Option<String>,
}

struct RuntimeTurnRecorder {
    turn_id: String,
    next_sequence: u64,
    final_phase: Option<String>,
}

impl RuntimeTurnRecorder {
    fn new() -> Self {
        Self {
            turn_id: format!("turn-{}", unix_timestamp_nanos()),
            next_sequence: 0,
            final_phase: None,
        }
    }

    fn record_phase(
        &mut self,
        bootstrap: &BootstrapReport,
        session_id: &str,
        phase: &str,
        step: Option<usize>,
        detail: serde_json::Value,
    ) -> Result<()> {
        self.next_sequence += 1;
        let payload = json!({
            "turn_id": self.turn_id,
            "sequence": self.next_sequence,
            "phase": phase,
            "step": step,
            "detail": detail,
        })
        .to_string();
        let logged = vela_state::append_event_to_session(
            &bootstrap.persistence.state_db_path,
            session_id,
            "runtime_turn_phase",
            payload,
        )?;
        if !logged {
            bail!(
                "failed to persist runtime turn lifecycle phase {:?} for session {:?}",
                phase,
                session_id
            );
        }
        self.final_phase = Some(phase.to_string());
        Ok(())
    }

    fn phase_count(&self) -> usize {
        self.next_sequence as usize
    }

    fn final_phase(&self) -> &str {
        self.final_phase.as_deref().unwrap_or("unknown")
    }
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
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone)]
enum ProviderContinuation {
    FinalAnswer,
    ToolRequest(RuntimeToolInvocation),
    InvalidToolRequest,
    EmptyResponse,
}

enum ReflectionOutcome {
    RetryPrompt(String),
    Fallback(RenderedChatResponse),
}

fn render_chat_response(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    request: &SessionRequest,
    provider_override: Option<&str>,
    model_override: Option<&str>,
    lifecycle: &mut RuntimeTurnRecorder,
) -> Result<RenderedChatResponse> {
    let execution = resolve_runtime_execution(
        &bootstrap.resolved_config,
        provider_override,
        model_override,
    )?;

    let memory = vela_memory::render_prompt_snapshot(&bootstrap.vela_home)?;
    let skills = vela_skills::list_skills(&bootstrap.vela_home)?;
    let reviews = vela_review::list_candidates(&bootstrap.vela_home)?;
    let compression_summary = vela_state::latest_compression_summary(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
    )?;
    let compression_block = compression_summary
        .as_deref()
        .map(|summary| format!("\n\nCompressed continuity summary:\n{}", summary))
        .unwrap_or_default();
    let memory_lines = memory.lines().count();

    if request.image_present {
        let image_path = request
            .image_path
            .as_deref()
            .unwrap_or("(unspecified image path)");
        if let Some(provider) = execution.provider.as_deref() {
            if let Some(image_path) = request.image_path.as_deref() {
                provider.validate()?;
                let image_base64 = encode_image_as_base64(image_path)?;
                let user_prompt = request
                    .query_text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| "Please analyze the attached image and respond concisely with the most relevant details for the runtime session.".to_string());
                let prompt = format!(
                    "You are Vela, a Rust-first agentic OS kernel runtime.\n\nSession: {} ({})\nMemory snapshot:\n{}{}\n\nLoaded skills: {}\nPending review candidates: {}\n\nUser image request:\n{}\n\nAttached image name: {}\n\nSupported runtime tools:\n- memory_snapshot\n- list_skills\n- view_memory (JSON: {{\"tool\":\"view_memory\",\"target\":\"memory\"}} or {{\"tool\":\"view_memory\",\"target\":\"user\"}})\n- search_session_history (JSON: {{\"tool\":\"search_session_history\",\"query\":\"keyword\",\"limit\":3}})\n- view_skill (JSON: {{\"tool\":\"view_skill\",\"name\":\"skill-name\"}})\nIf you need one tool before answering, respond with ONLY valid JSON for exactly one supported tool. Otherwise answer directly.",
                    session.title,
                    session.session_id,
                    memory,
                    compression_block,
                    skills.len(),
                    reviews.len(),
                    user_prompt,
                    std::path::Path::new(image_path).file_name().and_then(|n| n.to_str()).unwrap_or("attachment"),
                );
                return execute_provider_turn(
                    bootstrap,
                    session,
                    provider,
                    &prompt,
                    Some(vec![image_base64]),
                    &memory,
                    &skills,
                    lifecycle,
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
        if let Some(provider) = execution.provider.as_deref() {
            provider.validate()?;
            let prompt = format!(
                "You are Vela, a Rust-first agentic OS kernel runtime.\n\nSession: {} ({})\nMemory snapshot:\n{}{}\n\nLoaded skills: {}\nPending review candidates: {}\n\nUser query:\n{}\n\nSupported runtime tools:\n- memory_snapshot\n- list_skills\n- view_memory (JSON: {{\"tool\":\"view_memory\",\"target\":\"memory\"}} or {{\"tool\":\"view_memory\",\"target\":\"user\"}})\n- search_session_history (JSON: {{\"tool\":\"search_session_history\",\"query\":\"keyword\",\"limit\":3}})\n- view_skill (JSON: {{\"tool\":\"view_skill\",\"name\":\"skill-name\"}})\nIf you need one tool before answering, respond with ONLY valid JSON for exactly one supported tool. Otherwise answer directly.",
                session.title,
                session.session_id,
                memory,
                compression_block,
                skills.len(),
                reviews.len(),
                query.trim(),
            );
            return execute_provider_turn(
                bootstrap, session, provider, &prompt, None, &memory, &skills, lifecycle,
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
        .or_else(|| {
            resolved
                .runtime_provider
                .as_ref()
                .map(|s| s.trim().to_ascii_lowercase())
        });
    let model = model_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| resolved.runtime_model.clone());
    let provider = match provider_label.as_deref() {
        Some("ollama") => Some(Box::new(OllamaRuntimeProvider {
            label: "ollama".to_string(),
            model: model.clone(),
            base_url: resolved
                .runtime_ollama_base_url
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:11434".to_string()),
        }) as Box<dyn RuntimeProviderBackend>),
        Some(other) => bail!("unsupported runtime provider {other:?}"),
        None => None,
    };

    Ok(RuntimeExecutionConfig {
        provider,
        provider_label,
        model,
    })
}

/// Records one reflection attempt and returns either a retry prompt or a deterministic fallback.
fn handle_reflection_outcome(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    lifecycle: &mut RuntimeTurnRecorder,
    reflection_attempts: &mut usize,
    reason: &str,
    detail: serde_json::Value,
    fallback_message: &str,
    prompt_rewrite: String,
) -> Result<ReflectionOutcome> {
    *reflection_attempts += 1;
    let reflection_step = Some(*reflection_attempts);
    if *reflection_attempts > MAX_RUNTIME_REFLECTION_ATTEMPTS {
        lifecycle.record_phase(
            bootstrap,
            &session.session_id,
            "reflect",
            reflection_step,
            json!({"attempt": *reflection_attempts, "reason": reason, "detail": detail, "outcome": "fallback"}),
        )?;
        return Ok(ReflectionOutcome::Fallback(RenderedChatResponse {
            content: Some(fallback_message.to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
        }));
    }
    record_reflection_and_retry(
        bootstrap,
        session,
        lifecycle,
        *reflection_attempts,
        reflection_step,
        reason,
        detail,
    )?;
    Ok(ReflectionOutcome::RetryPrompt(prompt_rewrite))
}

/// Executes one provider-backed runtime turn and optionally completes a bounded local tool loop.
fn execute_provider_turn(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    provider: &dyn RuntimeProviderBackend,
    prompt: &str,
    images: Option<Vec<String>>,
    memory: &str,
    skills: &[vela_skills::SkillSummary],
    lifecycle: &mut RuntimeTurnRecorder,
) -> Result<RenderedChatResponse> {
    let mut current_prompt = prompt.to_string();
    let mut used_tool_loop = false;
    let mut reflection_attempts = 0usize;
    let mut tool_step = 0usize;

    while tool_step < MAX_RUNTIME_TOOL_STEPS {
        let response = provider.generate(&current_prompt, images.clone())?;
        match classify_provider_continuation(&response) {
            ProviderContinuation::ToolRequest(tool_request) => {
                tool_step += 1;
                used_tool_loop = true;
                persist_runtime_tool_request(
                    bootstrap,
                    &session.session_id,
                    &tool_request,
                    tool_step,
                )?;
                lifecycle.record_phase(
                    bootstrap,
                    &session.session_id,
                    "tool-request",
                    Some(tool_step),
                    json!({"request": tool_request.metadata_json(), "provider": provider.label(), "model": provider.model()}),
                )?;
                let tool_result = execute_runtime_tool(bootstrap, &tool_request, memory, skills);
                persist_runtime_tool_result(
                    bootstrap,
                    &session.session_id,
                    &tool_request,
                    tool_step,
                    &tool_result,
                )?;
                lifecycle.record_phase(
                    bootstrap,
                    &session.session_id,
                    "tool-result",
                    Some(tool_step),
                    json!({"request": tool_request.metadata_json(), "result_length": tool_result.len()}),
                )?;
                if tool_result.trim().is_empty() {
                    match handle_reflection_outcome(
                        bootstrap,
                        session,
                        lifecycle,
                        &mut reflection_attempts,
                        "empty-tool-result",
                        json!({"request": tool_request.metadata_json()}),
                        "Vela could not recover from an empty intermediate tool result within the bounded retry limit, so it fell back to a deterministic runtime response.",
                        format!(
                            "{}\n\nThe tool result for {} was empty and unusable. Do not repeat the same failed continuation blindly. Either request a supported tool with ONLY valid JSON for one approved tool, or answer directly.",
                            current_prompt,
                            tool_request.display_name(),
                        ),
                    )? {
                        ReflectionOutcome::Fallback(outcome) => return Ok(outcome),
                        ReflectionOutcome::RetryPrompt(prompt_rewrite) => {
                            current_prompt = prompt_rewrite;
                            continue;
                        }
                    }
                }

                let followup_instruction = if tool_step == MAX_RUNTIME_TOOL_STEPS {
                    "You have reached the maximum number of tool steps. Answer the user directly without requesting another tool."
                } else {
                    "You may either request another supported tool with ONLY valid JSON for one approved tool, or answer directly."
                };
                current_prompt = format!(
                    "{}\n\nCompleted tool step {} of {}.\nTool result for {}:\n{}\n\n{}",
                    current_prompt,
                    tool_step,
                    MAX_RUNTIME_TOOL_STEPS,
                    tool_request.request_text(),
                    tool_result,
                    followup_instruction,
                );
            }
            ProviderContinuation::FinalAnswer => {
                return Ok(RenderedChatResponse {
                    content: Some(response),
                    source: if used_tool_loop {
                        provider.tool_loop_response_source()
                    } else {
                        provider.direct_response_source()
                    },
                    provider: Some(provider.label().to_string()),
                    model: provider.model().map(str::to_string),
                });
            }
            ProviderContinuation::InvalidToolRequest => {
                match handle_reflection_outcome(
                    bootstrap,
                    session,
                    lifecycle,
                    &mut reflection_attempts,
                    "invalid-tool-request",
                    json!({"response": response}),
                    "Vela received an invalid provider continuation and exhausted the bounded reflection limit, so it fell back to a deterministic runtime response.",
                    format!(
                        "{}\n\nYour previous reply requested an unsupported or malformed tool envelope. Only these tools are allowed: memory_snapshot, list_skills, view_memory, search_session_history, view_skill. If you need one tool, respond with ONLY valid JSON for exactly one of those tool contracts. Otherwise answer the user directly.",
                        current_prompt,
                    ),
                )? {
                    ReflectionOutcome::Fallback(outcome) => return Ok(outcome),
                    ReflectionOutcome::RetryPrompt(prompt_rewrite) => current_prompt = prompt_rewrite,
                }
            }
            ProviderContinuation::EmptyResponse => {
                match handle_reflection_outcome(
                    bootstrap,
                    session,
                    lifecycle,
                    &mut reflection_attempts,
                    "empty-provider-response",
                    json!({}),
                    "Vela received an empty provider continuation and exhausted the bounded reflection limit, so it fell back to a deterministic runtime response.",
                    format!(
                        "{}\n\nYour previous reply was empty and unusable. Either request one supported tool with ONLY valid JSON for memory_snapshot, list_skills, view_memory, search_session_history, or view_skill, or answer the user directly with non-empty text.",
                        current_prompt,
                    ),
                )? {
                    ReflectionOutcome::Fallback(outcome) => return Ok(outcome),
                    ReflectionOutcome::RetryPrompt(prompt_rewrite) => current_prompt = prompt_rewrite,
                }
            }
        }
    }

    let final_response = provider.generate(&current_prompt, images)?;
    match classify_provider_continuation(&final_response) {
        ProviderContinuation::FinalAnswer => Ok(RenderedChatResponse {
            content: Some(final_response),
            source: provider.tool_loop_response_source(),
            provider: Some(provider.label().to_string()),
            model: provider.model().map(str::to_string),
        }),
        ProviderContinuation::ToolRequest(_) => Ok(RenderedChatResponse {
            content: Some("Vela reached the maximum bounded tool steps and fell back to a deterministic runtime response instead of continuing indefinitely.".to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
        }),
        ProviderContinuation::InvalidToolRequest => Ok(RenderedChatResponse {
            content: Some("Vela received an invalid provider continuation after the bounded tool loop and fell back to a deterministic runtime response.".to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
        }),
        ProviderContinuation::EmptyResponse => Ok(RenderedChatResponse {
            content: Some("Vela received an empty provider continuation after the bounded tool loop and fell back to a deterministic runtime response.".to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
        }),
    }
}

fn classify_provider_continuation(response: &str) -> ProviderContinuation {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return ProviderContinuation::EmptyResponse;
    }
    let json_body = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let looks_like_tool_envelope = json_body.starts_with('{') || trimmed.starts_with("```json");
    let Ok(request) = serde_json::from_str::<RuntimeToolRequest>(json_body) else {
        return if looks_like_tool_envelope {
            ProviderContinuation::InvalidToolRequest
        } else {
            ProviderContinuation::FinalAnswer
        };
    };
    let tool = match request.tool.trim() {
        "memory_snapshot" => RuntimeToolInvocation {
            name: RuntimeToolName::MemorySnapshot,
            target: None,
            query: None,
            skill_name: None,
            limit: None,
        },
        "list_skills" => RuntimeToolInvocation {
            name: RuntimeToolName::ListSkills,
            target: None,
            query: None,
            skill_name: None,
            limit: None,
        },
        "view_memory" => {
            let target = match request.target.as_deref() {
                Some(raw) => match vela_memory::MemoryTarget::parse(raw) {
                    Ok(target) => Some(target),
                    Err(_) => return ProviderContinuation::InvalidToolRequest,
                },
                None => Some(vela_memory::MemoryTarget::Memory),
            };
            RuntimeToolInvocation {
                name: RuntimeToolName::ViewMemory,
                target,
                query: None,
                skill_name: None,
                limit: None,
            }
        }
        "search_session_history" => {
            let query = request
                .query
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let Some(query) = query else {
                return ProviderContinuation::InvalidToolRequest;
            };
            RuntimeToolInvocation {
                name: RuntimeToolName::SearchSessionHistory,
                target: None,
                query: Some(query),
                skill_name: None,
                limit: request.limit.map(|value| value.clamp(1, 5)),
            }
        }
        "view_skill" => {
            let name = request
                .name
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let Some(skill_name) = name else {
                return ProviderContinuation::InvalidToolRequest;
            };
            RuntimeToolInvocation {
                name: RuntimeToolName::ViewSkill,
                target: None,
                query: None,
                skill_name: Some(skill_name),
                limit: None,
            }
        }
        _ => return ProviderContinuation::InvalidToolRequest,
    };
    ProviderContinuation::ToolRequest(tool)
}

fn record_reflection_and_retry(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    lifecycle: &mut RuntimeTurnRecorder,
    attempt: usize,
    step: Option<usize>,
    reason: &str,
    detail: serde_json::Value,
) -> Result<()> {
    lifecycle.record_phase(
        bootstrap,
        &session.session_id,
        "reflect",
        step,
        json!({
            "attempt": attempt,
            "reason": reason,
            "detail": detail,
        }),
    )?;
    lifecycle.record_phase(
        bootstrap,
        &session.session_id,
        "retry",
        step,
        json!({
            "attempt": attempt,
            "reason": reason,
        }),
    )?;
    Ok(())
}

/// Executes one approved read-only runtime tool and returns its textual result.
fn execute_runtime_tool(
    bootstrap: &BootstrapReport,
    tool: &RuntimeToolInvocation,
    memory_snapshot: &str,
    skills: &[vela_skills::SkillSummary],
) -> String {
    match tool.name {
        RuntimeToolName::MemorySnapshot => memory_snapshot.to_string(),
        RuntimeToolName::ListSkills => {
            if skills.is_empty() {
                "(no loaded skills)".to_string()
            } else {
                skills
                    .iter()
                    .map(|skill| match skill.description.as_deref() {
                        Some(description) => format!("{} — {}", skill.name, description),
                        None => skill.name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        RuntimeToolName::ViewMemory => {
            let target = tool.target.unwrap_or(vela_memory::MemoryTarget::Memory);
            match vela_memory::view_memory(&bootstrap.vela_home, target) {
                Ok(view) => {
                    if view.entries.is_empty() {
                        format!("{}: (no entries)", target.label())
                    } else {
                        format!("{}:\n{}", target.label(), view.entries.join("\n\n"))
                    }
                }
                Err(error) => format!("failed to load {}: {}", target.label(), error),
            }
        }
        RuntimeToolName::SearchSessionHistory => {
            let query = tool.query.as_deref().unwrap_or_default();
            let limit = tool.limit.unwrap_or(3);
            match vela_state::search_session_history(
                &bootstrap.persistence.state_db_path,
                query,
                limit,
            ) {
                Ok(hits) if hits.is_empty() => {
                    format!("session search for {:?}: no matches", query)
                }
                Ok(hits) => hits
                    .into_iter()
                    .map(|hit| format!("{} :: {}", hit.session_title, hit.snippet))
                    .collect::<Vec<_>>()
                    .join("\n"),
                Err(error) => format!(
                    "failed to search session history for {:?}: {}",
                    query, error
                ),
            }
        }
        RuntimeToolName::ViewSkill => {
            let name = tool.skill_name.as_deref().unwrap_or_default();
            match vela_skills::view_skill(&bootstrap.vela_home, name) {
                Ok(skill) => format!("skill {}:\n{}", skill.name, skill.content),
                Err(error) => format!("failed to view skill {:?}: {}", name, error),
            }
        }
    }
}

/// Persists the requested runtime tool before execution begins.
fn persist_runtime_tool_request(
    bootstrap: &BootstrapReport,
    session_id: &str,
    tool: &RuntimeToolInvocation,
    step: usize,
) -> Result<()> {
    let metadata =
        json!({"source": "runtime-tool-loop", "step": step, "request": tool.metadata_json()})
            .to_string();
    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "runtime_tool_requested",
        metadata.clone(),
    )?;
    if !event_logged {
        bail!(
            "failed to persist runtime tool request event for session {:?}",
            session_id
        );
    }
    let message_logged = vela_state::append_message_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "tool-request",
        &tool.request_text(),
        Some(metadata),
    )?;
    if !message_logged {
        bail!(
            "failed to persist runtime tool request message for session {:?}",
            session_id
        );
    }
    Ok(())
}

/// Persists the completed runtime tool result and its metadata.
fn persist_runtime_tool_result(
    bootstrap: &BootstrapReport,
    session_id: &str,
    tool: &RuntimeToolInvocation,
    step: usize,
    result: &str,
) -> Result<()> {
    let metadata = json!({"source": "runtime-tool-loop", "step": step, "request": tool.metadata_json(), "result_length": result.len()}).to_string();
    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "runtime_tool_completed",
        metadata.clone(),
    )?;
    if !event_logged {
        bail!(
            "failed to persist runtime tool completion event for session {:?}",
            session_id
        );
    }
    let message_logged = vela_state::append_message_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "tool-result",
        result,
        Some(metadata),
    )?;
    if !message_logged {
        bail!(
            "failed to persist runtime tool result message for session {:?}",
            session_id
        );
    }
    Ok(())
}

fn call_ollama_generate(
    base_url: &str,
    model: &str,
    prompt: &str,
    images: Option<Vec<String>>,
) -> Result<String> {
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
    let payload: OllamaGenerateResponse = response
        .json()
        .context("failed to decode Ollama response")?;
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
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
    {
        return Ok(());
    }

    let parsed = reqwest::Url::parse(base_url)
        .with_context(|| format!("invalid Ollama base URL {:?}", base_url))?;
    let host = parsed
        .host_str()
        .context("Ollama base URL is missing a host")?;
    let is_local = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|ip| {
                ip.is_loopback()
                    || ip == IpAddr::V4(Ipv4Addr::LOCALHOST)
                    || ip == IpAddr::V6(Ipv6Addr::LOCALHOST)
            })
            .unwrap_or(false);

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates an isolated bootstrap report for runtime regression tests.
    fn test_bootstrap(prefix: &str) -> BootstrapReport {
        let vela_home =
            std::env::temp_dir().join(format!("vela-runtime-{prefix}-{}", unix_timestamp_nanos()));
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
            extensions: vela_extensions::initialize_extensions(
                &vela_home,
                &ResolvedConfig::default(),
            )
            .unwrap(),
        }
    }

    #[test]
    /// Verifies that runtime execution resolves the Ollama backend through the provider boundary.
    fn resolve_runtime_execution_wraps_ollama_provider_backend() {
        let resolved = ResolvedConfig {
            runtime_provider: Some("ollama".to_string()),
            runtime_model: Some("gemma3:4b".to_string()),
            runtime_ollama_base_url: Some("http://127.0.0.1:11434".to_string()),
            ..ResolvedConfig::default()
        };

        let execution = resolve_runtime_execution(&resolved, None, None).unwrap();
        let provider = execution
            .provider
            .as_deref()
            .expect("resolved provider backend");

        assert_eq!(execution.provider_label.as_deref(), Some("ollama"));
        assert_eq!(execution.model.as_deref(), Some("gemma3:4b"));
        assert_eq!(provider.label(), "ollama");
        assert_eq!(provider.model(), Some("gemma3:4b"));
        assert_eq!(provider.direct_response_source(), "runtime-ollama");
        assert_eq!(
            provider.tool_loop_response_source(),
            "runtime-ollama-tool-loop"
        );
        provider.validate().unwrap();
    }

    #[test]
    /// Verifies that extension reload re-reads config and manifests without resetting durable session state.
    fn reload_extensions_rereads_config_without_resetting_sessions() {
        let vela_home = std::env::temp_dir().join(format!(
            "vela-runtime-ext-reload-{}",
            unix_timestamp_nanos()
        ));
        let _ = std::fs::remove_dir_all(&vela_home);
        std::fs::create_dir_all(vela_home.join("extensions")).unwrap();
        std::fs::write(
            vela_home.join("extensions").join("demo.yaml"),
            "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\nentry: extensions/demo-tool.wasm\ncapabilities:\n  - chat\n",
        )
        .unwrap();
        std::fs::write(
            vela_home.join("config.yaml"),
            "extensions:\n  entries:\n    demo:\n      enabled: false\n",
        )
        .unwrap();

        let (_, resolved_config) = vela_config::reload_config_snapshot(&vela_home, false).unwrap();
        let bootstrap = BootstrapReport {
            vela_home: vela_home.clone(),
            active_profile: None,
            loaded_env_paths: vec![],
            ignored_user_config: false,
            config_sources: vec![],
            resolved_config,
            persistence: vela_state::initialize_persistence(&vela_home).unwrap(),
            memory: vela_memory::initialize_memory(&vela_home).unwrap(),
            skills: vela_skills::initialize_skills(&vela_home).unwrap(),
            reviews: vela_review::initialize_reviews(&vela_home).unwrap(),
            extensions: vela_extensions::initialize_extensions(
                &vela_home,
                &vela_config::reload_config_snapshot(&vela_home, false)
                    .unwrap()
                    .1,
            )
            .unwrap(),
        };
        assert_eq!(bootstrap.extensions.activated_count, 0);
        assert_eq!(bootstrap.extensions.disabled_count, 1);

        let session = resolve_runtime_session(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("reload test".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
        )
        .unwrap();
        let before = current_session_summary(&bootstrap)
            .unwrap()
            .expect("session before reload");
        assert_eq!(before.id, session.session_id);

        std::fs::write(vela_home.join("config.yaml"), "extensions: {}\n").unwrap();
        let reloaded = reload_extensions(&bootstrap).unwrap();
        let after = current_session_summary(&bootstrap)
            .unwrap()
            .expect("session after reload");

        assert_eq!(reloaded.activated_count, 1);
        assert_eq!(reloaded.disabled_count, 0);
        assert_eq!(before.id, after.id);
        assert_eq!(before.title, after.title);

        std::fs::write(
            vela_home.join("extensions").join("demo.yaml"),
            "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\ncapabilities:\n  - chat\n",
        )
        .unwrap();
        let failed = reload_extensions(&bootstrap).unwrap();
        let after_failed = current_session_summary(&bootstrap)
            .unwrap()
            .expect("session after failed reload");
        assert_eq!(failed.failed_count, 1);
        assert_eq!(failed.activated_count, 0);
        assert_eq!(before.id, after_failed.id);
        assert_eq!(before.title, after_failed.title);

        let _ = std::fs::remove_dir_all(&vela_home);
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
            assert!(
                read > 0,
                "mock Ollama request closed before full payload arrived"
            );
            request_bytes.extend_from_slice(&buf[..read]);
            if let Some(end) = request_bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let end = end + 4;
                let head = String::from_utf8_lossy(&request_bytes[..end]).into_owned();
                let content_length = head
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("Content-Length: ")
                            .or_else(|| line.strip_prefix("content-length: "))
                    })
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
            let read = stream
                .read(&mut buf)
                .expect("read mock Ollama request body");
            assert!(read > 0, "mock Ollama request closed before body finished");
            request_bytes.extend_from_slice(&buf[..read]);
        }
        String::from_utf8_lossy(&request_bytes[..expected_total_len]).into_owned()
    }

    fn assert_mock_ollama_request(request: &str, exchange: &MockOllamaExchange<'_>) {
        let (head, body_text) = request.split_once("\r\n\r\n").expect("split HTTP request");
        let request_line = head.lines().next().expect("HTTP request line");
        assert!(request_line.starts_with("POST /api/generate HTTP/1.1"));
        let payload_json: serde_json::Value =
            serde_json::from_str(body_text).expect("decode request body");
        assert_eq!(
            payload_json.get("model").and_then(|v| v.as_str()),
            Some(exchange.expected_model)
        );
        assert_eq!(
            payload_json.get("stream").and_then(|v| v.as_bool()),
            Some(false)
        );
        let prompt = payload_json
            .get("prompt")
            .and_then(|v| v.as_str())
            .expect("prompt field");
        assert!(prompt.contains(exchange.prompt_fragment));
        let images = payload_json.get("images").and_then(|v| v.as_array());
        if let Some(expected_image_base64) = exchange.expected_image_base64 {
            let images = images.expect("images field");
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].as_str(), Some(expected_image_base64));
        } else {
            assert!(
                payload_json.get("images").is_none(),
                "images field should be absent when no image is expected"
            );
        }
    }

    fn spawn_mock_ollama_sequence(
        exchanges: Vec<MockOllamaExchange<'static>>,
    ) -> (String, std::thread::JoinHandle<()>) {
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
        let added =
            add_scheduled_job(&bootstrap, "*/5 * * * *", "ping status", Some("test")).unwrap();
        let setup = setup_scheduler(&bootstrap).unwrap();
        let lock = acquire_scheduler_jobs_lock(&setup.jobs_path).unwrap();
        let mut jobs = load_scheduler_jobs(&setup.jobs_path).unwrap();
        let job = jobs
            .iter_mut()
            .find(|job| job.id == added.id)
            .expect("scheduler job");
        job.next_run_at = unix_timestamp() - 1;
        save_scheduler_jobs(&setup.jobs_path, &jobs).unwrap();
        drop(lock);
        let second = start_scheduler(&bootstrap).unwrap();

        assert_eq!(first.session.session_id, second.session.session_id);
        assert_eq!(second.setup.job_count, 1);
        assert_eq!(second.executed_job_count, 1);
        let summary = current_command_session_summary(&bootstrap, "cron")
            .unwrap()
            .expect("cron session summary");
        assert!(summary.message_count >= first_summary.message_count + 2);
        assert!(summary.event_count >= first_summary.event_count + 4);

        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies scheduler execution reschedules completed jobs and recovers stale running jobs safely.
    fn scheduler_executes_and_recovers_jobs() {
        let bootstrap = test_bootstrap("scheduler-exec-recover");
        let added =
            add_scheduled_job(&bootstrap, "* * * * *", "ping status", Some("test")).unwrap();
        let setup = setup_scheduler(&bootstrap).unwrap();
        let lock = acquire_scheduler_jobs_lock(&setup.jobs_path).unwrap();
        let mut jobs = load_scheduler_jobs(&setup.jobs_path).unwrap();
        let job = jobs
            .iter_mut()
            .find(|job| job.id == added.id)
            .expect("scheduler job");
        job.next_run_at = unix_timestamp() - 1;
        save_scheduler_jobs(&setup.jobs_path, &jobs).unwrap();
        drop(lock);

        let first = start_scheduler(&bootstrap).unwrap();
        assert_eq!(first.executed_job_count, 1);
        assert_eq!(first.recovered_job_count, 0);
        assert_eq!(first.failed_job_count, 0);
        let first_job = get_scheduled_job(&bootstrap, &added.id).unwrap();
        assert_eq!(first_job.status, "pending");
        assert_eq!(first_job.run_count, 1);
        assert_eq!(first_job.last_outcome.as_deref(), Some("completed"));
        assert!(first_job.next_run_at > first_job.created_at);

        let setup = setup_scheduler(&bootstrap).unwrap();
        let lock = acquire_scheduler_jobs_lock(&setup.jobs_path).unwrap();
        let mut jobs = load_scheduler_jobs(&setup.jobs_path).unwrap();
        let job = jobs
            .iter_mut()
            .find(|job| job.id == added.id)
            .expect("scheduler job");
        let stale_started_at = unix_timestamp() - SCHEDULER_RECOVERY_LEASE_SECONDS - 1;
        job.status = "running".to_string();
        job.last_started_at = Some(stale_started_at);
        job.execution_token = Some("stale-attempt".to_string());
        job.lease_expires_at = Some(unix_timestamp() - 1);
        job.next_run_at = unix_timestamp() - 1;
        save_scheduler_jobs(&setup.jobs_path, &jobs).unwrap();
        drop(lock);

        let second = start_scheduler(&bootstrap).unwrap();
        assert_eq!(second.executed_job_count, 1);
        assert_eq!(second.recovered_job_count, 1);
        assert_eq!(second.failed_job_count, 0);
        let recovered_job = get_scheduled_job(&bootstrap, &added.id).unwrap();
        assert_eq!(recovered_job.status, "pending");
        assert_eq!(recovered_job.run_count, 2);
        assert_eq!(recovered_job.recovery_count, 1);
        assert_eq!(recovered_job.last_outcome.as_deref(), Some("completed"));
        assert!(recovered_job.last_recovered_at.is_some());

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

        assert!(report
            .response
            .as_deref()
            .unwrap_or_default()
            .contains("Vela executed a local kernel turn"));
        assert!(report.turn_id.starts_with("turn-"));
        assert_eq!(report.lifecycle_phase_count, 4);
        assert_eq!(report.final_phase, "finish");
        assert_eq!(report.emitted_signal_count, 1);
        assert_eq!(report.generated_candidate_count, 1);
        let summary = current_session_summary(&bootstrap)
            .unwrap()
            .expect("chat session summary");
        assert_eq!(summary.message_count, 2);
        let inspection = inspect_latest_session(&bootstrap, 20)
            .unwrap()
            .expect("chat session inspection");
        let lifecycle: Vec<_> = inspection
            .lifecycle
            .iter()
            .filter(|record| record.turn_id == report.turn_id)
            .map(|record| record.phase.as_str())
            .collect();
        assert_eq!(
            lifecycle,
            vec!["receive", "deliberate", "respond", "finish"]
        );
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

        assert!(report
            .response
            .as_deref()
            .unwrap_or_default()
            .contains("Vela executed a local image turn"));
        let summary = current_session_summary(&bootstrap)
            .unwrap()
            .expect("image chat session summary");
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

        assert_eq!(
            report.response.as_deref(),
            Some("Gemma inspected the image.")
        );
        assert_eq!(report.response_source, "runtime-ollama");
        let inspection = inspect_latest_session(&bootstrap, 10)
            .unwrap()
            .expect("image session inspection");
        let assistant = inspection.messages.last().expect("assistant message");
        let metadata: serde_json::Value = serde_json::from_str(
            assistant
                .metadata_json
                .as_deref()
                .expect("assistant metadata"),
        )
        .expect("decode assistant metadata");
        assert_eq!(
            metadata.get("source").and_then(|v| v.as_str()),
            Some("runtime-ollama")
        );
        assert_eq!(
            metadata.get("provider").and_then(|v| v.as_str()),
            Some("ollama")
        );
        assert_eq!(
            metadata.get("model").and_then(|v| v.as_str()),
            Some("gemma3:4b")
        );
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

        assert_eq!(
            report.response.as_deref(),
            Some("Gemma handled both prompt and image.")
        );
        assert_eq!(report.response_source, "runtime-ollama");
        server.join().unwrap();
        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that configured Ollama execution is used for text chat turns.
    fn execute_chat_turn_uses_ollama_provider_when_configured() {
        let (base_url, server) =
            spawn_mock_ollama("Gemma says hi.", "gemma3:4b", "hello there", None);
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
    /// Verifies that a configured provider turn can retrieve targeted memory, session, and skill context through runtime tools.
    fn execute_chat_turn_retrieves_targeted_internal_context() {
        let (base_url, server) = spawn_mock_ollama_sequence(vec![
            MockOllamaExchange {
                response_body: r#"{"tool":"view_memory","target":"user"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "retrieve targeted context",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: r#"{"tool":"search_session_history","query":"retrieve targeted context","limit":2}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "Tool result for view_memory:user:",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: r#"{"tool":"view_skill","name":"deploy-staging"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment:
                    "Tool result for search_session_history:retrieve targeted context:",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: "Context-aware final answer.",
                expected_model: "gemma3:4b",
                prompt_fragment: "Tool result for view_skill:deploy-staging:",
                expected_image_base64: None,
            },
        ]);
        let mut bootstrap = test_bootstrap("ollama-context-tools");
        bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
        bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
        bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
        vela_memory::add_memory_entry(
            &bootstrap.vela_home,
            vela_memory::MemoryTarget::User,
            "Prefers terse answers.",
        )
        .unwrap();
        std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
        std::fs::write(
            bootstrap
                .vela_home
                .join("skills")
                .join("deploy-staging")
                .join("SKILL.md"),
            "# deploy-staging\n\nDeploy staging safely.",
        )
        .unwrap();

        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("retrieve targeted context".to_string()),
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

        assert_eq!(
            report.response.as_deref(),
            Some("Context-aware final answer.")
        );
        assert_eq!(report.response_source, "runtime-ollama-tool-loop");
        let inspection = inspect_latest_session(&bootstrap, 20)
            .unwrap()
            .expect("context tool inspection");
        assert_eq!(inspection.messages[1].content, "view_memory:user");
        assert!(inspection.messages[2]
            .content
            .contains("Prefers terse answers."));
        assert_eq!(
            inspection.messages[3].content,
            "search_session_history:retrieve targeted context"
        );
        assert!(inspection.messages[4].content.contains("retrieve"));
        assert_eq!(inspection.messages[5].content, "view_skill:deploy-staging");
        assert!(inspection.messages[6]
            .content
            .contains("Deploy staging safely."));
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
                prompt_fragment:
                    "Completed tool step 2 of 3.\nTool result for list_skills:\ndeploy-staging",
                expected_image_base64: None,
            },
        ]);
        let mut bootstrap = test_bootstrap("ollama-tool-loop");
        bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
        bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
        bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
        std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
        std::fs::write(
            bootstrap
                .vela_home
                .join("skills")
                .join("deploy-staging")
                .join("SKILL.md"),
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

        assert_eq!(
            report.response.as_deref(),
            Some("Tool-informed final answer.")
        );
        assert_eq!(report.response_source, "runtime-ollama-tool-loop");
        assert_eq!(report.lifecycle_phase_count, 8);
        assert_eq!(report.final_phase, "finish");
        let inspection = inspect_latest_session(&bootstrap, 20)
            .unwrap()
            .expect("tool loop inspection");
        assert_eq!(inspection.messages.len(), 6);
        assert_eq!(inspection.messages[1].role, "tool-request");
        assert_eq!(inspection.messages[1].content, "memory_snapshot");
        assert_eq!(inspection.messages[2].role, "tool-result");
        assert!(!inspection.messages[2].content.trim().is_empty());
        let first_tool_result_metadata: serde_json::Value = serde_json::from_str(
            inspection.messages[2]
                .metadata_json
                .as_deref()
                .expect("first tool-result metadata"),
        )
        .expect("decode first tool-result metadata");
        assert_eq!(
            first_tool_result_metadata
                .get("request")
                .and_then(|v| v.get("tool"))
                .and_then(|v| v.as_str()),
            Some("memory_snapshot")
        );
        assert_eq!(
            first_tool_result_metadata
                .get("step")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(inspection.messages[3].role, "tool-request");
        assert_eq!(inspection.messages[3].content, "list_skills");
        assert_eq!(inspection.messages[4].role, "tool-result");
        assert!(inspection.messages[4].content.contains("deploy-staging"));
        assert_eq!(
            inspection
                .events
                .iter()
                .filter(|event| event.event_type == "runtime_tool_requested")
                .count(),
            2
        );
        assert_eq!(
            inspection
                .events
                .iter()
                .filter(|event| event.event_type == "runtime_tool_completed")
                .count(),
            2
        );
        let lifecycle: Vec<_> = inspection
            .lifecycle
            .iter()
            .filter(|record| record.turn_id == report.turn_id)
            .map(|record| (record.phase.as_str(), record.step))
            .collect();
        assert_eq!(
            lifecycle,
            vec![
                ("receive", None),
                ("deliberate", None),
                ("tool-request", Some(1)),
                ("tool-result", Some(1)),
                ("tool-request", Some(2)),
                ("tool-result", Some(2)),
                ("respond", None),
                ("finish", None),
            ]
        );
        let assistant = inspection.messages.last().expect("assistant message");
        let metadata: serde_json::Value = serde_json::from_str(
            assistant
                .metadata_json
                .as_deref()
                .expect("assistant metadata"),
        )
        .expect("decode assistant metadata");
        assert_eq!(
            metadata.get("source").and_then(|v| v.as_str()),
            Some("runtime-ollama-tool-loop")
        );
        server.join().unwrap();
        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that the runtime can reflect on an invalid tool request and recover with a bounded retry.
    fn execute_chat_turn_reflects_and_recovers_from_invalid_tool_request() {
        let (base_url, server) = spawn_mock_ollama_sequence(vec![
            MockOllamaExchange {
                response_body: r#"{"tool":"shell_exec"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "recover from invalid tool",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: "Recovered final answer.",
                expected_model: "gemma3:4b",
                prompt_fragment: "unsupported or malformed tool envelope",
                expected_image_base64: None,
            },
        ]);
        let mut bootstrap = test_bootstrap("ollama-reflect-recover");
        bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
        bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
        bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);

        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("recover from invalid tool".to_string()),
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

        assert_eq!(report.response.as_deref(), Some("Recovered final answer."));
        assert_eq!(report.response_source, "runtime-ollama");
        assert_eq!(report.lifecycle_phase_count, 6);
        let inspection = inspect_latest_session(&bootstrap, 20)
            .unwrap()
            .expect("reflection inspection");
        let lifecycle: Vec<_> = inspection
            .lifecycle
            .iter()
            .filter(|record| record.turn_id == report.turn_id)
            .map(|record| (record.phase.as_str(), record.step))
            .collect();
        assert_eq!(
            lifecycle,
            vec![
                ("receive", None),
                ("deliberate", None),
                ("reflect", Some(1)),
                ("retry", Some(1)),
                ("respond", None),
                ("finish", None),
            ]
        );
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
                prompt_fragment:
                    "Completed tool step 2 of 3.\nTool result for list_skills:\ndeploy-staging",
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
            bootstrap
                .vela_home
                .join("skills")
                .join("deploy-staging")
                .join("SKILL.md"),
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
        assert_eq!(report.lifecycle_phase_count, 10);
        assert_eq!(report.final_phase, "finish");
        assert!(report
            .response
            .as_deref()
            .unwrap_or_default()
            .contains("maximum bounded tool steps"));
        let inspection = inspect_latest_session(&bootstrap, 20)
            .unwrap()
            .expect("max-step inspection");
        assert_eq!(inspection.messages.len(), 8);
        assert_eq!(
            inspection
                .events
                .iter()
                .filter(|event| event.event_type == "runtime_tool_requested")
                .count(),
            3
        );
        assert_eq!(
            inspection
                .events
                .iter()
                .filter(|event| event.event_type == "runtime_tool_completed")
                .count(),
            3
        );
        let third_tool_result_metadata: serde_json::Value = serde_json::from_str(
            inspection.messages[6]
                .metadata_json
                .as_deref()
                .expect("third tool-result metadata"),
        )
        .expect("decode third tool-result metadata");
        assert_eq!(inspection.messages[6].role, "tool-result");
        assert_eq!(
            third_tool_result_metadata
                .get("request")
                .and_then(|v| v.get("tool"))
                .and_then(|v| v.as_str()),
            Some("memory_snapshot")
        );
        assert_eq!(
            third_tool_result_metadata
                .get("step")
                .and_then(|v| v.as_u64()),
            Some(3)
        );
        let lifecycle: Vec<_> = inspection
            .lifecycle
            .iter()
            .filter(|record| record.turn_id == report.turn_id)
            .map(|record| (record.sequence, record.phase.as_str(), record.step))
            .collect();
        assert_eq!(
            lifecycle,
            vec![
                (1, "receive", None),
                (2, "deliberate", None),
                (3, "tool-request", Some(1)),
                (4, "tool-result", Some(1)),
                (5, "tool-request", Some(2)),
                (6, "tool-result", Some(2)),
                (7, "tool-request", Some(3)),
                (8, "tool-result", Some(3)),
                (9, "respond", None),
                (10, "finish", None),
            ]
        );
        let assistant = inspection.messages.last().expect("assistant message");
        let assistant_metadata: serde_json::Value = serde_json::from_str(
            assistant
                .metadata_json
                .as_deref()
                .expect("assistant metadata"),
        )
        .expect("decode assistant metadata");
        assert_eq!(
            assistant_metadata.get("source").and_then(|v| v.as_str()),
            Some("runtime-kernel")
        );
        assert_eq!(
            assistant_metadata.get("provider").and_then(|v| v.as_str()),
            None
        );
        assert_eq!(
            assistant_metadata.get("model").and_then(|v| v.as_str()),
            None
        );
        server.join().unwrap();
        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }

    #[test]
    /// Verifies that repeated invalid provider continuations fall back after the bounded reflection limit.
    fn execute_chat_turn_falls_back_after_exhausting_reflection_retries() {
        let (base_url, server) = spawn_mock_ollama_sequence(vec![
            MockOllamaExchange {
                response_body: r#"{"tool":"shell_exec"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "exhaust reflection retries",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: r#"{"tool":"shell_exec"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "unsupported or malformed tool envelope",
                expected_image_base64: None,
            },
            MockOllamaExchange {
                response_body: r#"{"tool":"shell_exec"}"#,
                expected_model: "gemma3:4b",
                prompt_fragment: "unsupported or malformed tool envelope",
                expected_image_base64: None,
            },
        ]);
        let mut bootstrap = test_bootstrap("ollama-reflect-fallback");
        bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
        bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
        bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);

        let report = execute_chat_turn(
            &bootstrap,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("exhaust reflection retries".to_string()),
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
            .contains("exhausted the bounded reflection limit"));
        assert_eq!(report.lifecycle_phase_count, 9);
        let inspection = inspect_latest_session(&bootstrap, 20)
            .unwrap()
            .expect("reflection fallback inspection");
        let lifecycle: Vec<_> = inspection
            .lifecycle
            .iter()
            .filter(|record| record.turn_id == report.turn_id)
            .map(|record| (record.phase.as_str(), record.step))
            .collect();
        assert_eq!(
            lifecycle,
            vec![
                ("receive", None),
                ("deliberate", None),
                ("reflect", Some(1)),
                ("retry", Some(1)),
                ("reflect", Some(2)),
                ("retry", Some(2)),
                ("reflect", Some(3)),
                ("respond", None),
                ("finish", None),
            ]
        );
        server.join().unwrap();
        std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
    }
}
