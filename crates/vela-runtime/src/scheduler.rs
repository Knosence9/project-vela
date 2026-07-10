use super::*;

/// Ensures the durable scheduler directory, config, and job registry exist.
pub fn setup_scheduler(bootstrap: &BootstrapReport) -> Result<SchedulerSetupReport> {
    let scheduler_dir = bootstrap.vela_home.join("scheduler");
    std::fs::create_dir_all(&scheduler_dir)?;

    let config_path = scheduler_dir.join("config.json");
    let jobs_path = scheduler_dir.join("jobs.json");
    let config_existed_before = config_path.is_file();
    let jobs_existed_before = jobs_path.is_file();

    if !config_existed_before {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config_path)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(
                    serde_json::to_string_pretty(&json!({
                        "version": 1,
                        "default_source": "scheduler",
                        "session_command_name": "cron",
                        "active_profile": bootstrap.active_profile,
                        "transport_mode": "local-bootstrap",
                    }))?
                    .as_bytes(),
                )?;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    if !jobs_existed_before {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&jobs_path)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(b"[]")?;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
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
    let mut session = resolve_runtime_session(bootstrap, request)?;
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
                        "provider_capabilities": rendered.provider_capability_summary,
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
                "provider_capabilities": rendered.provider_capability_summary,
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

        session.runtime_state = lifecycle.final_phase().to_string();
        Ok(ChatTurnReport {
            session: session.clone(),
            turn_id: lifecycle.turn_id.clone(),
            response: rendered.content,
            response_source: rendered.source.to_string(),
            response_provider: rendered.provider,
            response_model: rendered.model,
            response_provider_capabilities: rendered.provider_capability_summary,
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
    delivery_webhook_url: Option<&str>,
    delivery_event_type: Option<&str>,
) -> Result<ScheduledJob> {
    let setup = setup_scheduler(bootstrap)?;
    let schedule = normalize_scheduler_schedule(schedule)?;
    let task = normalize_scheduler_task(task)?;
    let source = normalize_scheduler_source(source);
    let delivery_webhook_url = normalize_scheduler_delivery_webhook_url(delivery_webhook_url)?;
    let delivery_event_type = if delivery_webhook_url.is_some() {
        Some(
            normalize_scheduler_delivery_event_type(delivery_event_type)
                .unwrap_or_else(|| "scheduler.job.outcome".to_string()),
        )
    } else {
        None
    };
    let lock = acquire_scheduler_jobs_lock(&setup.jobs_path)?;
    let mut jobs = load_scheduler_jobs(&setup.jobs_path)?;
    if jobs.iter().any(|job| {
        job.schedule == schedule
            && job.task == task
            && job.source == source
            && job.delivery_webhook_url == delivery_webhook_url
            && job.delivery_event_type == delivery_event_type
            && matches!(job.status.as_str(), "pending" | "running" | "failed")
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
        last_progression: Some("registered".to_string()),
        last_error: None,
        run_count: 0,
        recovery_count: 0,
        last_session_id: None,
        execution_token: None,
        lease_expires_at: None,
        delivery_webhook_url,
        delivery_event_type,
        last_delivery_at: None,
        last_delivery_outcome: None,
        last_delivery_error: None,
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
            job.last_progression = Some("recovered-for-retry".to_string());
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
    job.last_progression = Some("started-attempt".to_string());
    job.last_error = None;
    job.execution_token = Some(execution_token.clone());
    job.lease_expires_at = Some(now + SCHEDULER_RECOVERY_LEASE_SECONDS);
    let task = job.task.clone();
    let schedule = job.schedule.clone();
    let delivery_webhook_url = job.delivery_webhook_url.clone();
    let delivery_event_type = job.delivery_event_type.clone();
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
                job.last_progression = Some("completed-rescheduled".to_string());
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
            maybe_deliver_scheduler_job_outcome(
                bootstrap,
                scheduler_session_id,
                jobs_path,
                job_id,
                &execution_token,
                delivery_webhook_url.as_deref(),
                delivery_event_type.as_deref(),
                json!({
                    "job_id": job_id,
                    "schedule": schedule,
                    "task": task,
                    "outcome": "completed",
                    "completed_at": completed_at,
                    "response_source": report.response_source,
                    "response": report.response,
                    "response_provider": report.response_provider,
                    "response_model": report.response_model,
                    "session_id": report.session.session_id,
                }),
            );
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
                job.last_progression = Some("failed-rescheduled".to_string());
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
            maybe_deliver_scheduler_job_outcome(
                bootstrap,
                scheduler_session_id,
                jobs_path,
                job_id,
                &execution_token,
                delivery_webhook_url.as_deref(),
                delivery_event_type.as_deref(),
                json!({
                    "job_id": job_id,
                    "schedule": schedule,
                    "task": task,
                    "outcome": "failed",
                    "failed_at": failed_at,
                    "error": error.to_string(),
                }),
            );
            Err(error)
        }
    }
}

fn maybe_deliver_scheduler_job_outcome(
    bootstrap: &BootstrapReport,
    scheduler_session_id: &str,
    jobs_path: &std::path::Path,
    job_id: &str,
    execution_token: &str,
    delivery_webhook_url: Option<&str>,
    delivery_event_type: Option<&str>,
    payload: serde_json::Value,
) {
    let Some(delivery_webhook_url) = delivery_webhook_url else {
        return;
    };

    let result = (|| -> Result<()> {
        let payload_text = serde_json::to_string(&payload)?;
        let attempted_at = unix_timestamp();
        match deliver_gateway_webhook(
            bootstrap,
            delivery_webhook_url,
            &payload_text,
            delivery_event_type,
        ) {
            Ok(report) => {
                let lock = acquire_scheduler_jobs_lock(jobs_path)?;
                let mut jobs = load_scheduler_jobs(jobs_path)?;
                if let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) {
                    job.updated_at = attempted_at;
                    job.last_delivery_at = Some(attempted_at);
                    job.last_delivery_outcome = Some("delivered".to_string());
                    job.last_delivery_error = None;
                }
                save_scheduler_jobs(jobs_path, &jobs)?;
                drop(lock);

                let event_logged = vela_state::append_event_to_session(
                    &bootstrap.persistence.state_db_path,
                    scheduler_session_id,
                    "scheduler_job_delivery_completed",
                    json!({
                        "job_id": job_id,
                        "delivery_at": attempted_at,
                        "execution_token": execution_token,
                        "delivery_session_id": report.session.session_id,
                        "event_type": report.event_type,
                        "url": report.url,
                        "status_code": report.status_code,
                        "outbox_record_path": report.outbox_record_path,
                    })
                    .to_string(),
                )?;
                if !event_logged {
                    tracing::warn!(session_id=%scheduler_session_id, job_id=%job_id, "failed to append scheduler job delivery completion event");
                }
            }
            Err(error) => {
                let error_text = error.to_string();
                let lock = acquire_scheduler_jobs_lock(jobs_path)?;
                let mut jobs = load_scheduler_jobs(jobs_path)?;
                if let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) {
                    job.updated_at = attempted_at;
                    job.last_delivery_at = Some(attempted_at);
                    job.last_delivery_outcome = Some("failed".to_string());
                    job.last_delivery_error = Some(error_text.clone());
                }
                save_scheduler_jobs(jobs_path, &jobs)?;
                drop(lock);

                let event_logged = vela_state::append_event_to_session(
                    &bootstrap.persistence.state_db_path,
                    scheduler_session_id,
                    "scheduler_job_delivery_failed",
                    json!({
                        "job_id": job_id,
                        "delivery_at": attempted_at,
                        "execution_token": execution_token,
                        "url": delivery_webhook_url,
                        "event_type": delivery_event_type.unwrap_or("scheduler.job.outcome"),
                        "error": error_text,
                    })
                    .to_string(),
                )?;
                if !event_logged {
                    tracing::warn!(session_id=%scheduler_session_id, job_id=%job_id, "failed to append scheduler job delivery failure event");
                }
            }
        }
        Ok(())
    })();

    if let Err(error) = result {
        tracing::warn!(session_id=%scheduler_session_id, job_id=%job_id, error=%error, "scheduler delivery bookkeeping failed after job execution settled");
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
