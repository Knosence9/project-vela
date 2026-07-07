use crate::cli::{AgentsArgs, CronArgs, EvalArgs, GatewayArgs, McpArgs, SessionsArgs};
use anyhow::Result;

fn scheduler_job_last_run_at(job: &vela_runtime::ScheduledJob) -> Option<i64> {
    [
        job.last_started_at,
        job.last_completed_at,
        job.last_failed_at,
    ]
    .into_iter()
    .flatten()
    .max()
}

fn scheduler_job_last_error_excerpt(job: &vela_runtime::ScheduledJob) -> Option<String> {
    job.last_error.as_ref().map(|error| {
        let single_line = error.replace('\n', " ");
        if single_line.chars().count() > 80 {
            format!("{}…", single_line.chars().take(80).collect::<String>())
        } else {
            single_line
        }
    })
}

pub(crate) fn run_gateway(
    bootstrap: &vela_runtime::BootstrapReport,
    args: &GatewayArgs,
) -> Result<()> {
    if args.start {
        let report = vela_runtime::start_gateway(bootstrap)?;
        println!(
            "gateway started: session={} action={} title={} config={}",
            report.session.session_id,
            report.session.action.label(),
            report.session.title,
            report.setup.config_path.display(),
        );
    } else if args.setup {
        let report = vela_runtime::setup_gateway(bootstrap)?;
        println!(
            "gateway setup: dir={} config={} existed_before={} inbox={} outbox={}",
            report.gateway_dir.display(),
            report.config_path.display(),
            report.config_existed_before,
            report.inbox_dir.display(),
            report.outbox_dir.display(),
        );
    } else if let Some(url) = args.webhook_url.as_deref() {
        let payload = args
            .payload
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--payload is required with --webhook-url"))?;
        let report = vela_runtime::deliver_gateway_webhook(
            bootstrap,
            url,
            payload,
            args.event_type.as_deref(),
        )?;
        println!(
            "gateway webhook delivered: session={} action={} status={} event={} url={} outbox={}",
            report.session.session_id,
            report.session.action.label(),
            report.status_code,
            report.event_type,
            report.url,
            report.outbox_record_path.display(),
        );
    } else {
        let report = vela_runtime::setup_gateway(bootstrap)?;
        match vela_runtime::current_command_session_summary(bootstrap, "gateway")? {
            Some(session) => println!(
                "gateway ready: config={} session={} title={} messages={} events={}",
                report.config_path.display(),
                session.id,
                session.title,
                session.message_count,
                session.event_count,
            ),
            None => println!(
                "gateway ready: config={} session=none inbox={} outbox={}",
                report.config_path.display(),
                report.inbox_dir.display(),
                report.outbox_dir.display(),
            ),
        }
    }
    Ok(())
}

pub(crate) fn run_sessions(
    bootstrap: &vela_runtime::BootstrapReport,
    args: &SessionsArgs,
) -> Result<()> {
    if let Some(source) = args.branch.as_deref() {
        let branch = vela_runtime::branch_session(
            bootstrap,
            source,
            args.title.as_deref(),
            args.note.as_deref(),
        )?;
        println!(
            "session branched: session={} title={} parent={} note={:?}",
            branch.session_id,
            branch.title,
            branch.parent_session_id.as_deref().unwrap_or("none"),
            branch.branch_note,
        );
    } else if let Some(target) = args.compress.as_deref() {
        let summary = args
            .summary
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--summary is required with --compress"))?;
        let compression = vela_runtime::compress_session(bootstrap, target, summary)?;
        println!(
            "session compressed: session={} compression={} messages={} events={} summary={}",
            compression.session_id,
            compression.id,
            compression.source_message_count,
            compression.source_event_count,
            compression.summary,
        );
    } else if let Some(target) = args.show.as_deref() {
        match vela_runtime::inspect_session(bootstrap, target, 20)? {
            Some(inspection) => {
                println!(
                    "session inspect: id={} title={} parent_id={:?} parent_title={:?} branch_note={:?} messages={} events={}",
                    inspection.session_id,
                    inspection.title,
                    inspection.branch.parent_session_id,
                    inspection.branch.parent_title,
                    inspection.branch.branch_note,
                    inspection.messages.len(),
                    inspection.events.len(),
                );
                if inspection.child_sessions.is_empty() {
                    println!("children: none");
                } else {
                    println!("children [{}]:", inspection.child_sessions.len());
                    for child in inspection.child_sessions {
                        println!(
                            "- session={} title={} messages={} events={} parent={:?}",
                            child.id,
                            child.title,
                            child.message_count,
                            child.event_count,
                            child.parent_session_id,
                        );
                    }
                }
                if inspection.compressions.is_empty() {
                    println!("compressions: none");
                } else {
                    println!("compressions [{}]:", inspection.compressions.len());
                    for compression in inspection.compressions {
                        println!(
                            "- {} :: messages={} events={} summary={}",
                            compression.id,
                            compression.source_message_count,
                            compression.source_event_count,
                            compression.summary,
                        );
                    }
                }
            }
            None => println!("session inspect: not found for {:?}", target),
        }
    } else if let Some(query) = args.search.as_deref() {
        let hits = vela_runtime::search_session_history(bootstrap, query, 10)?;
        if hits.is_empty() {
            println!("session search: no hits for {:?}", query);
        } else {
            println!("session search hits for {:?}:", query);
            for hit in hits {
                println!(
                    "- session={} title={} message={} snippet={}",
                    hit.session_id, hit.session_title, hit.message_id, hit.snippet
                );
            }
        }
    } else {
        println!(
            "sessions placeholder: list={} browse={}",
            args.list, args.browse
        );
    }
    Ok(())
}

pub(crate) fn run_agents(
    bootstrap: &vela_runtime::BootstrapReport,
    args: &AgentsArgs,
) -> Result<()> {
    if let Some(task) = args.delegate.as_deref() {
        let role = args
            .role
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--role is required with --delegate"))?;
        let report =
            vela_runtime::request_subagent_delegation(bootstrap, role, task, args.note.as_deref())?;
        println!(
            "delegation requested: id={} role={} status={} session={} task={} note={:?}",
            report.record.id,
            report.record.role,
            report.record.status,
            report.session.session_id,
            report.record.task,
            report.record.note,
        );
    } else if args.list {
        let records = vela_runtime::list_subagent_delegations(bootstrap)?;
        println!("delegations [{}]:", records.len());
        for record in records {
            println!(
                "- {} :: role={} status={} created_at={} updated_at={} session={} task={} note={:?}",
                record.id,
                record.role,
                record.status,
                record.created_at,
                record.updated_at,
                record.session_id,
                record.task,
                record.note,
            );
        }
    } else if let Some(id) = args.show.as_deref() {
        match vela_runtime::get_subagent_delegation(bootstrap, id)? {
            Some(record) => println!(
                "delegation: id={} role={} status={} created_at={} updated_at={} session={} task={} note={:?}",
                record.id,
                record.role,
                record.status,
                record.created_at,
                record.updated_at,
                record.session_id,
                record.task,
                record.note,
            ),
            None => println!("delegation: not found for {:?}", id),
        }
    } else {
        let setup = vela_runtime::setup_subagent_delegations(bootstrap)?;
        match vela_runtime::current_command_session_summary(bootstrap, "agents")? {
            Some(session) => println!(
                "agents ready: dir={} delegations={} session={} title={} messages={} events={}",
                setup.agents_dir.display(),
                setup.delegation_count,
                session.id,
                session.title,
                session.message_count,
                session.event_count,
            ),
            None => println!(
                "agents ready: dir={} delegations={} session=none file={}",
                setup.agents_dir.display(),
                setup.delegation_count,
                setup.delegations_path.display(),
            ),
        }
    }
    Ok(())
}

pub(crate) fn run_mcp(bootstrap: &vela_runtime::BootstrapReport, args: &McpArgs) -> Result<()> {
    if let Some(server) = args.bridge.as_deref() {
        let tool = args
            .tool
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--tool is required with --bridge"))?;
        let payload = args
            .payload
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--payload is required with --bridge"))?;
        let report = vela_runtime::request_mcp_bridge_call(
            bootstrap,
            server,
            tool,
            payload,
            args.note.as_deref(),
        )?;
        println!(
            "mcp bridge requested: id={} server={} tool={} status={} session={} payload={} note={:?}",
            report.record.id,
            report.record.server,
            report.record.tool,
            report.record.status,
            report.session.session_id,
            report.record.payload,
            report.record.note,
        );
    } else if args.list {
        let records = vela_runtime::list_mcp_bridge_calls(bootstrap)?;
        println!("mcp bridge requests [{}]:", records.len());
        for record in records {
            println!(
                "- {} :: server={} tool={} status={} created_at={} updated_at={} session={} payload={} note={:?}",
                record.id,
                record.server,
                record.tool,
                record.status,
                record.created_at,
                record.updated_at,
                record.session_id,
                record.payload,
                record.note,
            );
        }
    } else if let Some(id) = args.show.as_deref() {
        match vela_runtime::get_mcp_bridge_call(bootstrap, id)? {
            Some(record) => println!(
                "mcp bridge request: id={} server={} tool={} status={} created_at={} updated_at={} session={} payload={} note={:?}",
                record.id,
                record.server,
                record.tool,
                record.status,
                record.created_at,
                record.updated_at,
                record.session_id,
                record.payload,
                record.note,
            ),
            None => println!("mcp bridge request: not found for {:?}", id),
        }
    } else {
        let setup = vela_runtime::setup_mcp_bridge(bootstrap)?;
        match vela_runtime::current_command_session_summary(bootstrap, "mcp")? {
            Some(session) => println!(
                "mcp ready: dir={} requests={} session={} title={} messages={} events={}",
                setup.mcp_dir.display(),
                setup.request_count,
                session.id,
                session.title,
                session.message_count,
                session.event_count,
            ),
            None => println!(
                "mcp ready: dir={} requests={} session=none file={}",
                setup.mcp_dir.display(),
                setup.request_count,
                setup.requests_path.display(),
            ),
        }
    }
    Ok(())
}

pub(crate) fn run_eval(bootstrap: &vela_runtime::BootstrapReport, args: &EvalArgs) -> Result<()> {
    if let Some(prompt) = args.run.as_deref() {
        let report = vela_runtime::run_backend_eval(
            bootstrap,
            prompt,
            &args.backends,
            args.model.as_deref(),
        )?;
        println!(
            "backend eval run: id={} session={} slot={:?} backends={} results={} parity_summary={:?} prompt={:?}",
            report.record.id,
            report.session.session_id,
            report.record.experiment_slot,
            report.record.backends.join(","),
            report.record.results.len(),
            report.record.parity_summary,
            report.record.prompt,
        );
        for result in &report.record.results {
            println!(
                "- backend={} transport={} status={} duration_ms={} source={:?} model={:?} response_chars={} capabilities={:?} error={:?} preview={:?}",
                result.backend_id,
                result.transport,
                result.status,
                result.duration_ms,
                result.response_source,
                result.response_model,
                result.response_chars,
                result.provider_capabilities,
                result.error,
                result.response_preview,
            );
        }
    } else if let Some(slot) = args.run_slot.as_deref() {
        let report = vela_runtime::run_backend_eval_slot(
            bootstrap,
            slot,
            &args.backends,
            args.model.as_deref(),
        )?;
        println!(
            "backend eval run: id={} session={} slot={:?} backends={} results={} parity_summary={:?} prompt={:?}",
            report.record.id,
            report.session.session_id,
            report.record.experiment_slot,
            report.record.backends.join(","),
            report.record.results.len(),
            report.record.parity_summary,
            report.record.prompt,
        );
        for result in &report.record.results {
            println!(
                "- backend={} transport={} status={} duration_ms={} source={:?} model={:?} response_chars={} capabilities={:?} error={:?} preview={:?}",
                result.backend_id,
                result.transport,
                result.status,
                result.duration_ms,
                result.response_source,
                result.response_model,
                result.response_chars,
                result.provider_capabilities,
                result.error,
                result.response_preview,
            );
        }
    } else if args.list {
        let runs = vela_runtime::list_backend_evals(bootstrap)?;
        println!("backend eval runs [{}]:", runs.len());
        for run in runs {
            println!(
                "- {} :: created_at={} session={} slot={:?} backends={} results={} parity_summary={:?} prompt={:?}",
                run.id,
                run.created_at,
                run.session_id,
                run.experiment_slot,
                run.backends.join(","),
                run.results.len(),
                run.parity_summary,
                run.prompt,
            );
        }
    } else if let Some(id) = args.show.as_deref() {
        match vela_runtime::get_backend_eval(bootstrap, id)? {
            Some(run) => {
                println!(
                    "backend eval: id={} created_at={} session={} slot={:?} backends={} results={} parity_summary={:?} prompt={:?}",
                    run.id,
                    run.created_at,
                    run.session_id,
                    run.experiment_slot,
                    run.backends.join(","),
                    run.results.len(),
                    run.parity_summary,
                    run.prompt,
                );
                for result in run.results {
                    println!(
                        "- backend={} transport={} status={} duration_ms={} source={:?} model={:?} response_chars={} capabilities={:?} error={:?} preview={:?}",
                        result.backend_id,
                        result.transport,
                        result.status,
                        result.duration_ms,
                        result.response_source,
                        result.response_model,
                        result.response_chars,
                        result.provider_capabilities,
                        result.error,
                        result.response_preview,
                    );
                }
            }
            None => println!("backend eval: not found for {:?}", id),
        }
    } else if args.list_slots {
        let slots = vela_runtime::list_backend_experiment_slots(bootstrap)?;
        println!("backend experiment slots [{}]:", slots.len());
        for slot in slots {
            println!(
                "- {} :: status={} strategy={} backends={} prompt={:?} summary={:?}",
                slot.id,
                slot.status,
                slot.strategy,
                slot.allowed_backends.join(","),
                slot.default_prompt,
                slot.summary,
            );
        }
    } else if let Some(id) = args.show_slot.as_deref() {
        match vela_runtime::get_backend_experiment_slot(bootstrap, id)? {
            Some(slot) => println!(
                "backend experiment slot: id={} status={} strategy={} backends={} prompt={:?} summary={:?} hypothesis={:?}",
                slot.id,
                slot.status,
                slot.strategy,
                slot.allowed_backends.join(","),
                slot.default_prompt,
                slot.summary,
                slot.hypothesis,
            ),
            None => println!("backend experiment slot: not found for {:?}", id),
        }
    } else if args.show_policy {
        let policy = vela_runtime::get_model_lab_policy(bootstrap)?;
        println!(
            "model lab policy: version={} summary={:?}",
            policy.version, policy.summary,
        );
        println!("graduation gates [{}]:", policy.graduation_gates.len());
        for gate in policy.graduation_gates {
            println!("- {}", gate);
        }
        println!(
            "allowed strategies [{}]: {}",
            policy.allowed_experiment_strategies.len(),
            policy.allowed_experiment_strategies.join(",")
        );
        println!(
            "prohibited behaviors [{}]:",
            policy.prohibited_behaviors.len()
        );
        for item in policy.prohibited_behaviors {
            println!("- {}", item);
        }
        println!("required evidence [{}]:", policy.required_evidence.len());
        for item in policy.required_evidence {
            println!("- {}", item);
        }
    } else {
        let setup = vela_runtime::setup_backend_evals(bootstrap)?;
        match vela_runtime::current_command_session_summary(bootstrap, "eval")? {
            Some(session) => println!(
                "eval ready: dir={} runs={} slots={} policy={} session={} title={} messages={} events={}",
                setup.evals_dir.display(),
                setup.run_count,
                setup.slot_count,
                setup.policy_path.display(),
                session.id,
                session.title,
                session.message_count,
                session.event_count,
            ),
            None => println!(
                "eval ready: dir={} runs={} slots={} session=none file={} slots_file={} policy_file={}",
                setup.evals_dir.display(),
                setup.run_count,
                setup.slot_count,
                setup.runs_path.display(),
                setup.slots_path.display(),
                setup.policy_path.display(),
            ),
        }
    }
    Ok(())
}

pub(crate) fn run_cron(bootstrap: &vela_runtime::BootstrapReport, args: &CronArgs) -> Result<()> {
    if args.start {
        let report = vela_runtime::start_scheduler(bootstrap)?;
        println!(
            "scheduler started: session={} action={} title={} config={} jobs={} executed={} recovered={} failed={}",
            report.session.session_id,
            report.session.action.label(),
            report.session.title,
            report.setup.config_path.display(),
            report.setup.job_count,
            report.executed_job_count,
            report.recovered_job_count,
            report.failed_job_count,
        );
    } else if args.setup {
        let report = vela_runtime::setup_scheduler(bootstrap)?;
        println!(
            "scheduler setup: dir={} config={} jobs={} config_existed_before={} jobs_existed_before={} job_count={}",
            report.scheduler_dir.display(),
            report.config_path.display(),
            report.jobs_path.display(),
            report.config_existed_before,
            report.jobs_existed_before,
            report.job_count,
        );
    } else if args.list {
        let jobs = vela_runtime::list_scheduled_jobs(bootstrap)?;
        println!("scheduled jobs [{}]:", jobs.len());
        for job in jobs {
            println!(
                "- {} :: schedule={} source={} status={} next_run_at={} run_count={} recovery_count={} outcome={:?} progression={:?} last_error={:?} delivery_webhook_url={:?} delivery_event_type={:?} delivery_outcome={:?} delivery_error={:?} task={}",
                job.id, job.schedule, job.source, job.status, job.next_run_at, job.run_count, job.recovery_count, job.last_outcome, job.last_progression, job.last_error, job.delivery_webhook_url, job.delivery_event_type, job.last_delivery_outcome, job.last_delivery_error, job.task
            );
        }
    } else if args.report {
        let report = vela_runtime::setup_scheduler(bootstrap)?;
        let jobs = vela_runtime::list_scheduled_jobs(bootstrap)?;
        let pending_count = jobs.iter().filter(|job| job.status == "pending").count();
        let completed_count = jobs
            .iter()
            .filter(|job| job.last_outcome.as_deref() == Some("completed"))
            .count();
        let failed_count = jobs
            .iter()
            .filter(|job| job.last_outcome.as_deref() == Some("failed"))
            .count();
        let delivery_pending_count = jobs
            .iter()
            .filter(|job| job.delivery_webhook_url.is_some() && job.last_delivery_outcome.is_none())
            .count();
        let delivery_failed_count = jobs
            .iter()
            .filter(|job| job.last_delivery_outcome.as_deref() == Some("failed"))
            .count();
        let total_runs: u64 = jobs.iter().map(|job| job.run_count).sum();
        let total_recoveries: u64 = jobs.iter().map(|job| job.recovery_count).sum();
        let next_due = jobs.iter().min_by_key(|job| job.next_run_at);
        match vela_runtime::current_command_session_summary(bootstrap, "cron")? {
            Some(session) => println!(
                "scheduler report: config={} jobs={} pending={} completed={} failed={} delivery_pending={} delivery_failed={} total_runs={} total_recoveries={} next_due={:?} session={} title={} messages={} events={}",
                report.config_path.display(),
                jobs.len(),
                pending_count,
                completed_count,
                failed_count,
                delivery_pending_count,
                delivery_failed_count,
                total_runs,
                total_recoveries,
                next_due.map(|job| format!("{}@{}", job.id, job.next_run_at)),
                session.id,
                session.title,
                session.message_count,
                session.event_count,
            ),
            None => println!(
                "scheduler report: config={} jobs={} pending={} completed={} failed={} delivery_pending={} delivery_failed={} total_runs={} total_recoveries={} next_due={:?} session=none",
                report.config_path.display(),
                jobs.len(),
                pending_count,
                completed_count,
                failed_count,
                delivery_pending_count,
                delivery_failed_count,
                total_runs,
                total_recoveries,
                next_due.map(|job| format!("{}@{}", job.id, job.next_run_at)),
            ),
        }
        println!("scheduler jobs [{}]:", jobs.len());
        for job in &jobs {
            println!(
                "- {} :: status={} next_run_at={} last_run_at={:?} last_completed_at={:?} last_failed_at={:?} outcome={:?} progression={:?} run_count={} recovery_count={} delivery_outcome={:?} last_error_excerpt={:?} task={}",
                job.id,
                job.status,
                job.next_run_at,
                scheduler_job_last_run_at(job),
                job.last_completed_at,
                job.last_failed_at,
                job.last_outcome,
                job.last_progression,
                job.run_count,
                job.recovery_count,
                job.last_delivery_outcome,
                scheduler_job_last_error_excerpt(job),
                job.task
            );
        }
    } else if let Some(task) = args.add.as_deref() {
        let schedule = args
            .schedule
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--add requires --schedule <expr>"))?;
        let job = vela_runtime::add_scheduled_job(
            bootstrap,
            schedule,
            task,
            args.source.as_deref(),
            args.delivery_webhook_url.as_deref(),
            args.delivery_event_type.as_deref(),
        )?;
        println!(
            "scheduled job added: {} schedule={} source={} status={} next_run_at={} progression={:?} delivery_webhook_url={:?} delivery_event_type={:?} task={}",
            job.id, job.schedule, job.source, job.status, job.next_run_at, job.last_progression, job.delivery_webhook_url, job.delivery_event_type, job.task
        );
    } else if let Some(id) = args.show.as_deref() {
        let job = vela_runtime::get_scheduled_job(bootstrap, id)?;
        println!(
            "scheduled job: {} schedule={} source={} status={} created_at={} next_run_at={} run_count={} recovery_count={} outcome={:?} progression={:?} last_error={:?} delivery_webhook_url={:?} delivery_event_type={:?} delivery_at={:?} delivery_outcome={:?} delivery_error={:?} task={}",
            job.id, job.schedule, job.source, job.status, job.created_at, job.next_run_at, job.run_count, job.recovery_count, job.last_outcome, job.last_progression, job.last_error, job.delivery_webhook_url, job.delivery_event_type, job.last_delivery_at, job.last_delivery_outcome, job.last_delivery_error, job.task
        );
    } else {
        let report = vela_runtime::setup_scheduler(bootstrap)?;
        match vela_runtime::current_command_session_summary(bootstrap, "cron")? {
            Some(session) => println!(
                "scheduler ready: config={} jobs={} session={} title={} messages={} events={}",
                report.config_path.display(),
                report.job_count,
                session.id,
                session.title,
                session.message_count,
                session.event_count,
            ),
            None => println!(
                "scheduler ready: config={} jobs={} session=none dir={}",
                report.config_path.display(),
                report.job_count,
                report.scheduler_dir.display(),
            ),
        }
    }
    Ok(())
}
