use super::*;

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

pub(crate) const SCHEDULER_RECOVERY_LEASE_SECONDS: i64 = 300;

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
