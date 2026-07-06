use crate::cli::{AgentsArgs, CronArgs, GatewayArgs, McpArgs, SessionsArgs};
use anyhow::Result;

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
                "- {} :: schedule={} source={} status={} next_run_at={} run_count={} recovery_count={} outcome={:?} progression={:?} last_error={:?} task={}",
                job.id, job.schedule, job.source, job.status, job.next_run_at, job.run_count, job.recovery_count, job.last_outcome, job.last_progression, job.last_error, job.task
            );
        }
    } else if let Some(task) = args.add.as_deref() {
        let schedule = args
            .schedule
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--add requires --schedule <expr>"))?;
        let job =
            vela_runtime::add_scheduled_job(bootstrap, schedule, task, args.source.as_deref())?;
        println!(
            "scheduled job added: {} schedule={} source={} status={} next_run_at={} progression={:?} task={}",
            job.id, job.schedule, job.source, job.status, job.next_run_at, job.last_progression, job.task
        );
    } else if let Some(id) = args.show.as_deref() {
        let job = vela_runtime::get_scheduled_job(bootstrap, id)?;
        println!(
            "scheduled job: {} schedule={} source={} status={} created_at={} next_run_at={} run_count={} recovery_count={} outcome={:?} progression={:?} last_error={:?} task={}",
            job.id, job.schedule, job.source, job.status, job.created_at, job.next_run_at, job.run_count, job.recovery_count, job.last_outcome, job.last_progression, job.last_error, job.task
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
