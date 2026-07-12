use crate::cli::{AgentsArgs, CronArgs, EvalArgs, GatewayArgs, McpArgs, SessionsArgs};
use anyhow::Result;

fn scheduler_now_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

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

fn scheduler_single_line_excerpt(value: Option<&String>) -> Option<String> {
    value.map(|text| {
        let single_line = text.replace('\n', " ");
        if single_line.chars().count() > 80 {
            format!("{}…", single_line.chars().take(80).collect::<String>())
        } else {
            single_line
        }
    })
}

fn scheduler_job_last_error_excerpt(job: &vela_runtime::ScheduledJob) -> Option<String> {
    scheduler_single_line_excerpt(job.last_error.as_ref())
}

fn scheduler_job_last_delivery_error_excerpt(job: &vela_runtime::ScheduledJob) -> Option<String> {
    scheduler_single_line_excerpt(job.last_delivery_error.as_ref())
}

fn joined_values_or_none(values: &[String], delimiter: &str) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(delimiter)
    }
}

fn scheduler_job_due_state(job: &vela_runtime::ScheduledJob, now: i64) -> &'static str {
    if job.status == "running" {
        match job.lease_expires_at {
            Some(lease_expires_at) if lease_expires_at < now => "lease-expired",
            _ => "running",
        }
    } else if matches!(job.status.as_str(), "pending" | "failed") && job.next_run_at <= now {
        "overdue"
    } else {
        "scheduled"
    }
}

fn scheduler_job_health_lag_seconds(job: &vela_runtime::ScheduledJob, now: i64) -> Option<i64> {
    match scheduler_job_due_state(job, now) {
        "overdue" => Some(now - job.next_run_at),
        "lease-expired" => job
            .lease_expires_at
            .map(|lease_expires_at| now - lease_expires_at),
        _ => None,
    }
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
            "session compressed: session={} compression={} messages={} events={} delta_messages={} delta_events={} summary={}",
            compression.session_id,
            compression.id,
            compression.source_message_count,
            compression.source_event_count,
            compression.delta_message_count,
            compression.delta_event_count,
            compression.summary,
        );
    } else if let Some(target) = args.show.as_deref() {
        let selection = vela_runtime::inspect_session_selection(bootstrap, target, 20)?;
        println!(
            "session selection: target={:?} resolution={} anchor_id={:?} anchor_title={:?} resolved_id={:?} resolved_title={:?}",
            selection.target,
            selection.resolution,
            selection.anchor_session_id,
            selection.anchor_title,
            selection.resolved_session_id,
            selection.resolved_title,
        );
        match selection.inspection {
            Some(inspection) => {
                println!(
                    "session inspect: id={} title={} state={} parent_id={:?} parent_title={:?} branch_note={:?} messages={} events={}",
                    inspection.session_id,
                    inspection.title,
                    inspection.runtime_state,
                    inspection.branch.parent_session_id,
                    inspection.branch.parent_title,
                    inspection.branch.branch_note,
                    inspection.messages.len(),
                    inspection.events.len(),
                );
                if inspection.lineage.is_empty() {
                    println!("lineage: none");
                } else {
                    println!("lineage [{}]:", inspection.lineage.len());
                    for node in inspection.lineage {
                        println!(
                            "- depth={} session={} title={} state={} parent={:?} messages={} events={}",
                            node.depth,
                            node.session_id,
                            node.title,
                            node.runtime_state,
                            node.parent_session_id,
                            node.message_count,
                            node.event_count,
                        );
                    }
                }
                if inspection.child_sessions.is_empty() {
                    println!("children: none");
                } else {
                    println!("children [{}]:", inspection.child_sessions.len());
                    for child in inspection.child_sessions {
                        println!(
                            "- session={} title={} state={} messages={} events={} parent={:?}",
                            child.id,
                            child.title,
                            child.runtime_state,
                            child.message_count,
                            child.event_count,
                            child.parent_session_id,
                        );
                    }
                }
                if inspection.descendants.is_empty() {
                    println!("descendants: none");
                } else {
                    println!("descendants [{}]:", inspection.descendants.len());
                    for node in inspection.descendants {
                        println!(
                            "- depth={} session={} title={} state={} parent={:?} messages={} events={}",
                            node.depth,
                            node.session_id,
                            node.title,
                            node.runtime_state,
                            node.parent_session_id,
                            node.message_count,
                            node.event_count,
                        );
                    }
                }
                if inspection.compressions.is_empty() {
                    println!("compressions: none");
                } else {
                    println!("compressions [{}]:", inspection.compressions.len());
                    for compression in inspection.compressions {
                        println!(
                            "- {} :: messages={} events={} delta_messages={} delta_events={} summary={}",
                            compression.id,
                            compression.source_message_count,
                            compression.source_event_count,
                            compression.delta_message_count,
                            compression.delta_event_count,
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
    } else if args.list {
        let sessions = vela_runtime::list_sessions(bootstrap, 50)?;
        println!("sessions [{}]:", sessions.len());
        for session in sessions {
            println!(
                "- depth={} session={} title={} state={} parent={:?} messages={} events={}",
                session.depth,
                session.session_id,
                session.title,
                session.runtime_state,
                session.parent_session_id,
                session.message_count,
                session.event_count,
            );
        }
    } else if args.browse {
        let trees = vela_runtime::browse_session_branches(bootstrap, 20, 20)?;
        println!("session roots [{}]:", trees.len());
        for tree in trees {
            println!(
                "- root session={} title={} state={} messages={} events={}",
                tree.root.session_id,
                tree.root.title,
                tree.root.runtime_state,
                tree.root.message_count,
                tree.root.event_count,
            );
            if tree.descendants.is_empty() {
                println!("  descendants: none");
            } else {
                println!("  descendants [{}]:", tree.descendants.len());
                for node in tree.descendants {
                    println!(
                        "  - depth={} session={} title={} state={} parent={:?} messages={} events={}",
                        node.depth,
                        node.session_id,
                        node.title,
                        node.runtime_state,
                        node.parent_session_id,
                        node.message_count,
                        node.event_count,
                    );
                }
            }
        }
    } else {
        println!("sessions ready: use --list, --browse, --show, --search, --branch, or --compress");
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
            "backend eval run: id={} session={} slot={:?} backends={} results={} parity_summary={:?} score_summary={:?} prompt={:?}",
            report.record.id,
            report.session.session_id,
            report.record.experiment_slot,
            report.record.backends.join(","),
            report.record.results.len(),
            report.record.parity_summary,
            report.record.score_summary,
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
            "backend eval run: id={} session={} slot={:?} backends={} results={} parity_summary={:?} score_summary={:?} prompt={:?}",
            report.record.id,
            report.session.session_id,
            report.record.experiment_slot,
            report.record.backends.join(","),
            report.record.results.len(),
            report.record.parity_summary,
            report.record.score_summary,
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
                "- {} :: created_at={} session={} slot={:?} backends={} results={} parity_summary={:?} score_summary={:?} prompt={:?}",
                run.id,
                run.created_at,
                run.session_id,
                run.experiment_slot,
                run.backends.join(","),
                run.results.len(),
                run.parity_summary,
                run.score_summary,
                run.prompt,
            );
        }
    } else if let Some(id) = args.show.as_deref() {
        match vela_runtime::get_backend_eval(bootstrap, id)? {
            Some(run) => {
                println!(
                    "backend eval: id={} created_at={} session={} slot={:?} backends={} results={} parity_summary={:?} score_summary={:?} prompt={:?}",
                    run.id,
                    run.created_at,
                    run.session_id,
                    run.experiment_slot,
                    run.backends.join(","),
                    run.results.len(),
                    run.parity_summary,
                    run.score_summary,
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
        let slots = vela_runtime::inspect_backend_experiment_slots(bootstrap)?;
        println!("backend experiment slots [{}]:", slots.len());
        for inspection in slots {
            let latest_backend_evidence = if inspection.latest_backend_evidence.is_empty() {
                "none".to_string()
            } else {
                inspection.latest_backend_evidence.join("; ")
            };
            let slot = inspection.slot;
            println!(
                "- {} :: status={} strategy={} backends={} latest_eval_id={:?} latest_eval_at={:?} latest_passed={} latest_failed={} latest_capability_groups={} latest_results={} latest_parity_summary={:?} latest_score_summary={:?} latest_backend_evidence={} prompt={:?} summary={:?}",
                slot.id,
                slot.status,
                slot.strategy,
                slot.allowed_backends.join(","),
                inspection.latest_eval_id,
                inspection.latest_eval_created_at,
                joined_values_or_none(&inspection.latest_eval_passed_backends, ","),
                joined_values_or_none(&inspection.latest_eval_failed_backends, ","),
                joined_values_or_none(&inspection.latest_eval_capability_groups, " | "),
                inspection.latest_eval_result_count,
                inspection.latest_eval_parity_summary,
                inspection.latest_eval_score_summary,
                latest_backend_evidence,
                slot.default_prompt,
                slot.summary,
            );
        }
    } else if let Some(id) = args.show_slot.as_deref() {
        match vela_runtime::get_backend_experiment_slot_inspection(bootstrap, id)? {
            Some(inspection) => {
                let latest_backend_evidence = if inspection.latest_backend_evidence.is_empty() {
                    "none".to_string()
                } else {
                    inspection.latest_backend_evidence.join("; ")
                };
                let slot = inspection.slot;
                println!(
                    "backend experiment slot: id={} status={} strategy={} backends={} latest_eval_id={:?} latest_eval_at={:?} latest_backends={} latest_passed={} latest_failed={} latest_capability_groups={} latest_results={} latest_parity_summary={:?} latest_score_summary={:?} latest_backend_evidence={} prompt={:?} summary={:?} hypothesis={:?}",
                    slot.id,
                    slot.status,
                    slot.strategy,
                    slot.allowed_backends.join(","),
                    inspection.latest_eval_id,
                    inspection.latest_eval_created_at,
                    joined_values_or_none(&inspection.latest_eval_backends, ","),
                    joined_values_or_none(&inspection.latest_eval_passed_backends, ","),
                    joined_values_or_none(&inspection.latest_eval_failed_backends, ","),
                    joined_values_or_none(&inspection.latest_eval_capability_groups, " | "),
                    inspection.latest_eval_result_count,
                    inspection.latest_eval_parity_summary,
                    inspection.latest_eval_score_summary,
                    latest_backend_evidence,
                    slot.default_prompt,
                    slot.summary,
                    slot.hypothesis,
                )
            }
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
        println!(
            "adapter/fine-tune intake criteria [{}]:",
            policy.adapter_finetune_intake_criteria.len()
        );
        for item in policy.adapter_finetune_intake_criteria {
            println!("- {}", item);
        }
    } else {
        let setup = vela_runtime::setup_backend_evals(bootstrap)?;
        let default_backend =
            vela_runtime::resolve_runtime_backend_contract(&bootstrap.resolved_config, None)?
                .map(|contract| contract.id.to_string())
                .unwrap_or_else(|| "none".to_string());
        match vela_runtime::current_command_session_summary(bootstrap, "eval")? {
            Some(session) => println!(
                "eval ready: dir={} runs={} slots={} policy={} default_backend={} session={} title={} messages={} events={}",
                setup.evals_dir.display(),
                setup.run_count,
                setup.slot_count,
                setup.policy_path.display(),
                default_backend,
                session.id,
                session.title,
                session.message_count,
                session.event_count,
            ),
            None => println!(
                "eval ready: dir={} runs={} slots={} default_backend={} session=none file={} slots_file={} policy_file={}",
                setup.evals_dir.display(),
                setup.run_count,
                setup.slot_count,
                default_backend,
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
                "- {} :: schedule={} source={} status={} next_run_at={} run_count={} recovery_count={} missed_run_count={} last_missed_run_count={} outcome={:?} progression={:?} last_error={:?} delivery_webhook_url={:?} delivery_event_type={:?} delivery_outcome={:?} delivery_progression={:?} delivery_attempt_count={} delivery_status_code={:?} delivery_error={:?} task={}",
                job.id, job.schedule, job.source, job.status, job.next_run_at, job.run_count, job.recovery_count, job.missed_run_count, job.last_missed_run_count, job.last_outcome, job.last_progression, job.last_error, job.delivery_webhook_url, job.delivery_event_type, job.last_delivery_outcome, job.last_delivery_progression, job.delivery_attempt_count, job.last_delivery_status_code, job.last_delivery_error, job.task
            );
        }
    } else if args.report {
        let report = vela_runtime::setup_scheduler(bootstrap)?;
        let jobs = vela_runtime::list_scheduled_jobs(bootstrap)?;
        let now = scheduler_now_timestamp();
        let pending_count = jobs.iter().filter(|job| job.status == "pending").count();
        let running_count = jobs
            .iter()
            .filter(|job| scheduler_job_due_state(job, now) == "running")
            .count();
        let completed_count = jobs
            .iter()
            .filter(|job| job.last_outcome.as_deref() == Some("completed"))
            .count();
        let failed_count = jobs
            .iter()
            .filter(|job| job.last_outcome.as_deref() == Some("failed"))
            .count();
        let overdue_count = jobs
            .iter()
            .filter(|job| scheduler_job_due_state(job, now) == "overdue")
            .count();
        let lease_expired_count = jobs
            .iter()
            .filter(|job| scheduler_job_due_state(job, now) == "lease-expired")
            .count();
        let delivery_pending_count = jobs
            .iter()
            .filter(|job| job.last_delivery_progression.as_deref() == Some("delivery-pending"))
            .count();
        let delivery_failed_count = jobs
            .iter()
            .filter(|job| job.last_delivery_progression.as_deref() == Some("delivery-failed"))
            .count();
        let delivery_delivered_count = jobs
            .iter()
            .filter(|job| job.last_delivery_progression.as_deref() == Some("delivery-delivered"))
            .count();
        let delivery_skipped_count = jobs
            .iter()
            .filter(|job| job.last_delivery_progression.as_deref() == Some("delivery-skipped"))
            .count();
        let total_runs: u64 = jobs.iter().map(|job| job.run_count).sum();
        let total_recoveries: u64 = jobs.iter().map(|job| job.recovery_count).sum();
        let total_delivery_attempts: u64 = jobs.iter().map(|job| job.delivery_attempt_count).sum();
        let total_missed_runs: u64 = jobs.iter().map(|job| job.missed_run_count).sum();
        let next_due = jobs.iter().min_by_key(|job| job.next_run_at);
        match vela_runtime::current_command_session_summary(bootstrap, "cron")? {
            Some(session) => println!(
                "scheduler report: config={} jobs={} pending={} running={} completed={} failed={} overdue={} lease_expired={} delivery_pending={} delivery_failed={} delivery_delivered={} delivery_skipped={} total_runs={} total_recoveries={} total_missed_runs={} total_delivery_attempts={} next_due={:?} session={} title={} messages={} events={}",
                report.config_path.display(),
                jobs.len(),
                pending_count,
                running_count,
                completed_count,
                failed_count,
                overdue_count,
                lease_expired_count,
                delivery_pending_count,
                delivery_failed_count,
                delivery_delivered_count,
                delivery_skipped_count,
                total_runs,
                total_recoveries,
                total_missed_runs,
                total_delivery_attempts,
                next_due.map(|job| format!("{}@{}", job.id, job.next_run_at)),
                session.id,
                session.title,
                session.message_count,
                session.event_count,
            ),
            None => println!(
                "scheduler report: config={} jobs={} pending={} running={} completed={} failed={} overdue={} lease_expired={} delivery_pending={} delivery_failed={} delivery_delivered={} delivery_skipped={} total_runs={} total_recoveries={} total_missed_runs={} total_delivery_attempts={} next_due={:?} session=none",
                report.config_path.display(),
                jobs.len(),
                pending_count,
                running_count,
                completed_count,
                failed_count,
                overdue_count,
                lease_expired_count,
                delivery_pending_count,
                delivery_failed_count,
                delivery_delivered_count,
                delivery_skipped_count,
                total_runs,
                total_recoveries,
                total_missed_runs,
                total_delivery_attempts,
                next_due.map(|job| format!("{}@{}", job.id, job.next_run_at)),
            ),
        }
        println!("scheduler jobs [{}]:", jobs.len());
        for job in &jobs {
            let due_state = scheduler_job_due_state(job, now);
            let health_lag_seconds = scheduler_job_health_lag_seconds(job, now);
            let last_run_at = scheduler_job_last_run_at(job);
            let delivery_error_excerpt = scheduler_job_last_delivery_error_excerpt(job);
            let last_error_excerpt = scheduler_job_last_error_excerpt(job);
            println!(
                "- {id} :: schedule={schedule} source={source} status={status} updated_at={updated_at} next_run_at={next_run_at} due_state={due_state} health_lag_seconds={health_lag_seconds:?} lease_expires_at={lease_expires_at:?} last_run_at={last_run_at:?} last_completed_at={last_completed_at:?} last_failed_at={last_failed_at:?} last_recovered_at={last_recovered_at:?} last_session_id={last_session_id:?} execution_token={execution_token:?} outcome={outcome:?} progression={progression:?} run_count={run_count} recovery_count={recovery_count} missed_run_count={missed_run_count} last_missed_run_count={last_missed_run_count} delivery_at={delivery_at:?} delivery_event_type={delivery_event_type:?} delivery_outcome={delivery_outcome:?} delivery_progression={delivery_progression:?} delivery_attempt_count={delivery_attempt_count} delivery_status_code={delivery_status_code:?} delivery_error_excerpt={delivery_error_excerpt:?} last_error_excerpt={last_error_excerpt:?} task={task}",
                id = job.id,
                schedule = job.schedule,
                source = job.source,
                status = job.status,
                updated_at = job.updated_at,
                next_run_at = job.next_run_at,
                due_state = due_state,
                health_lag_seconds = health_lag_seconds,
                lease_expires_at = job.lease_expires_at,
                last_run_at = last_run_at,
                last_completed_at = job.last_completed_at,
                last_failed_at = job.last_failed_at,
                last_recovered_at = job.last_recovered_at,
                last_session_id = job.last_session_id,
                execution_token = job.execution_token,
                outcome = job.last_outcome,
                progression = job.last_progression,
                run_count = job.run_count,
                recovery_count = job.recovery_count,
                missed_run_count = job.missed_run_count,
                last_missed_run_count = job.last_missed_run_count,
                delivery_at = job.last_delivery_at,
                delivery_event_type = job.delivery_event_type,
                delivery_outcome = job.last_delivery_outcome,
                delivery_progression = job.last_delivery_progression,
                delivery_attempt_count = job.delivery_attempt_count,
                delivery_status_code = job.last_delivery_status_code,
                delivery_error_excerpt = delivery_error_excerpt,
                last_error_excerpt = last_error_excerpt,
                task = job.task,
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
            "scheduled job added: {} schedule={} source={} status={} next_run_at={} progression={:?} delivery_progression={:?} delivery_webhook_url={:?} delivery_event_type={:?} task={}",
            job.id, job.schedule, job.source, job.status, job.next_run_at, job.last_progression, job.last_delivery_progression, job.delivery_webhook_url, job.delivery_event_type, job.task
        );
    } else if let Some(id) = args.show.as_deref() {
        let job = vela_runtime::get_scheduled_job(bootstrap, id)?;
        println!(
            "scheduled job: {} schedule={} source={} status={} created_at={} updated_at={} next_run_at={} last_started_at={:?} last_completed_at={:?} last_failed_at={:?} last_recovered_at={:?} lease_expires_at={:?} last_session_id={:?} execution_token={:?} run_count={} recovery_count={} missed_run_count={} last_missed_run_count={} outcome={:?} progression={:?} last_error={:?} delivery_webhook_url={:?} delivery_event_type={:?} delivery_at={:?} delivery_outcome={:?} delivery_progression={:?} delivery_attempt_count={} delivery_status_code={:?} delivery_error={:?} task={}",
            job.id, job.schedule, job.source, job.status, job.created_at, job.updated_at, job.next_run_at, job.last_started_at, job.last_completed_at, job.last_failed_at, job.last_recovered_at, job.lease_expires_at, job.last_session_id, job.execution_token, job.run_count, job.recovery_count, job.missed_run_count, job.last_missed_run_count, job.last_outcome, job.last_progression, job.last_error, job.delivery_webhook_url, job.delivery_event_type, job.last_delivery_at, job.last_delivery_outcome, job.last_delivery_progression, job.delivery_attempt_count, job.last_delivery_status_code, job.last_delivery_error, job.task
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
