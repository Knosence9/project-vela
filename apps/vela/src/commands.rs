use crate::cli::{print_extension_record, Cli, Commands};
use anyhow::Result;

pub(crate) fn run_command(bootstrap: &vela_runtime::BootstrapReport, cli: &Cli) -> Result<()> {
    match &cli.command {
        Some(Commands::Chat(args)) => {
            let report = vela_runtime::execute_chat_turn(
                bootstrap,
                &vela_runtime::SessionRequest {
                    command_name: "chat".to_string(),
                    query_present: args.query.is_some(),
                    query_text: args.query.clone(),
                    image_present: args.image.is_some(),
                    image_path: args.image.clone(),
                    resume: args.resume.clone(),
                    continue_last: args.continue_last.clone(),
                },
                args.provider.as_deref(),
                args.model.as_deref(),
                args.checkpoints,
            )?;
            println!(
                "runtime session: action={} id={} title={} mode={}",
                report.session.action.label(),
                report.session.session_id,
                report.session.title,
                report.session.interaction_mode.label(),
            );
            if let Some(response) = report.response {
                println!("\n{}", response);
            }
            println!(
                "\nlifecycle: turn={} phases={} last={}",
                report.turn_id, report.lifecycle_phase_count, report.final_phase
            );
            if args.checkpoints {
                println!(
                    "\ncheckpoints: signals={} candidates={}",
                    report.emitted_signal_count, report.generated_candidate_count
                );
            }
        }
        None => {
            let report = vela_runtime::execute_chat_turn(
                bootstrap,
                &vela_runtime::SessionRequest {
                    command_name: "chat".to_string(),
                    query_present: false,
                    query_text: None,
                    image_present: false,
                    image_path: None,
                    resume: cli.resume.clone(),
                    continue_last: cli.continue_last.clone(),
                },
                None,
                None,
                false,
            )?;
            println!(
                "runtime session: action={} id={} title={} mode={}",
                report.session.action.label(),
                report.session.session_id,
                report.session.title,
                report.session.interaction_mode.label(),
            );
            if let Some(response) = report.response {
                println!("\n{}", response);
            }
            println!(
                "\nlifecycle: turn={} phases={} last={}",
                report.turn_id, report.lifecycle_phase_count, report.final_phase
            );
        }
        Some(Commands::Status) => {
            println!("{}", bootstrap.summary_line());
            if bootstrap.loaded_env_paths.is_empty() {
                println!("loaded env: none");
            } else {
                for path in &bootstrap.loaded_env_paths {
                    println!("loaded env: {}", path.display());
                }
            }
            for source in &bootstrap.config_sources {
                let detail = source
                    .detail
                    .as_deref()
                    .map(|d| format!(" :: {}", d))
                    .unwrap_or_default();
                println!(
                    "config source [{}]: {}{}",
                    source.kind.label(),
                    source.path.display(),
                    detail
                );
            }
            println!(
                "resolved config: display.interface={:?} hooks_auto_accept={:?} security.redact_secrets={:?} network.force_ipv4={:?} runtime.provider={:?} runtime.model={:?} runtime.ollama_base_url={:?}",
                bootstrap.resolved_config.display_interface,
                bootstrap.resolved_config.hooks_auto_accept,
                bootstrap.resolved_config.security_redact_secrets,
                bootstrap.resolved_config.network_force_ipv4,
                bootstrap.resolved_config.runtime_provider,
                bootstrap.resolved_config.runtime_model,
                bootstrap.resolved_config.runtime_ollama_base_url,
            );
            println!(
                "persistence: state_db={} existed_before={} bootstrap_runs={} sessions_dir={} snapshot_pattern={}",
                bootstrap.persistence.state_db_path.display(),
                bootstrap.persistence.state_db_existed_before,
                bootstrap.persistence.bootstrap_runs,
                bootstrap.persistence.sessions_dir.display(),
                bootstrap.persistence.snapshot_pattern,
            );
            println!(
                "memory: dir={} memory_file={} chars={}/{} existed_before={} user_file={} chars={}/{} existed_before={}",
                bootstrap.memory.memories_dir.display(),
                bootstrap.memory.memory_path.display(),
                bootstrap.memory.memory_char_count,
                bootstrap.memory.memory_char_limit,
                bootstrap.memory.memory_exists_before,
                bootstrap.memory.user_path.display(),
                bootstrap.memory.user_char_count,
                bootstrap.memory.user_char_limit,
                bootstrap.memory.user_exists_before,
            );
            println!(
                "skills: dir={} existed_before={} skill_count={}",
                bootstrap.skills.skills_dir.display(),
                bootstrap.skills.skills_dir_existed_before,
                bootstrap.skills.skill_count,
            );
            println!(
                "reviews: dir={} existed_before={} candidate_count={}",
                bootstrap.reviews.reviews_dir.display(),
                bootstrap.reviews.reviews_dir_existed_before,
                bootstrap.reviews.candidate_count,
            );
            println!("{}", bootstrap.extensions.summary_line());
            for entry in &bootstrap.extensions.entries {
                print_extension_record(entry);
            }
            match vela_runtime::current_session_summary(bootstrap)? {
                Some(summary) => println!(
                    "active session: id={} title={} messages={} events={}",
                    summary.id, summary.title, summary.message_count, summary.event_count
                ),
                None => println!("active session: none"),
            }
        }
        Some(Commands::Extensions(args)) => {
            if args.reload {
                let before = vela_runtime::current_session_summary(bootstrap)?;
                let report = vela_runtime::reload_extensions(bootstrap)?;
                let after = vela_runtime::current_session_summary(bootstrap)?;
                let session_preserved = match (before.as_ref(), after.as_ref()) {
                    (Some(before), Some(after)) => before.id == after.id,
                    (None, None) => true,
                    _ => false,
                };
                println!("extensions reloaded: {}", report.summary_line());
                println!(
                    "session preserved: {} before={:?} after={:?}",
                    session_preserved,
                    before.as_ref().map(|item| item.id.as_str()),
                    after.as_ref().map(|item| item.id.as_str()),
                );
                for entry in &report.entries {
                    print_extension_record(entry);
                }
            } else {
                println!("{}", bootstrap.extensions.summary_line());
                for entry in &bootstrap.extensions.entries {
                    print_extension_record(entry);
                }
            }
        }
        Some(Commands::Memory(args)) => {
            if args.prompt_snapshot {
                println!("{}", vela_runtime::render_memory_snapshot(bootstrap)?);
            } else if args.pending {
                let pending = vela_runtime::list_pending_memory(bootstrap)?;
                println!("pending memory writes [{}]:", pending.len());
                for item in pending {
                    println!(
                        "- {} :: action={} target={} old={:?} new={:?}",
                        item.id,
                        item.action,
                        item.target.label(),
                        item.old_text,
                        item.new_text
                    );
                }
            } else if let Some(id) = args.show.as_deref() {
                let item = vela_runtime::get_pending_memory(bootstrap, id)?;
                println!("{}", serde_json::to_string_pretty(&item)?);
            } else if let Some(id) = args.approve.as_deref() {
                let report = vela_runtime::approve_pending_memory(bootstrap, id)?;
                println!(
                    "memory approve: target={} entries={} chars={}/{}",
                    report.target.label(),
                    report.entry_count,
                    report.char_count,
                    report.char_limit
                );
            } else if let Some(id) = args.reject.as_deref() {
                vela_runtime::reject_pending_memory(bootstrap, id)?;
                println!("memory reject: {}", id);
            } else if let Some(content) = args.add.as_deref() {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                if args.stage {
                    let item = vela_runtime::stage_add_memory_entry(bootstrap, target, content)?;
                    println!(
                        "memory staged: {} action={} target={}",
                        item.id,
                        item.action,
                        item.target.label()
                    );
                } else {
                    let report = vela_runtime::add_memory_entry(bootstrap, target, content)?;
                    println!(
                        "memory {}: target={} entries={} chars={}/{}",
                        report.action,
                        report.target.label(),
                        report.entry_count,
                        report.char_count,
                        report.char_limit
                    );
                }
            } else if let Some(content) = args.replace.as_deref() {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                let old_text = args
                    .match_text
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--replace requires --match <substring>"))?;
                if args.stage {
                    let item = vela_runtime::stage_replace_memory_entry(
                        bootstrap, target, old_text, content,
                    )?;
                    println!(
                        "memory staged: {} action={} target={}",
                        item.id,
                        item.action,
                        item.target.label()
                    );
                } else {
                    let report =
                        vela_runtime::replace_memory_entry(bootstrap, target, old_text, content)?;
                    println!(
                        "memory {}: target={} entries={} chars={}/{}",
                        report.action,
                        report.target.label(),
                        report.entry_count,
                        report.char_count,
                        report.char_limit
                    );
                }
            } else if let Some(old_text) = args.remove.as_deref() {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                if args.stage {
                    let item =
                        vela_runtime::stage_remove_memory_entry(bootstrap, target, old_text)?;
                    println!(
                        "memory staged: {} action={} target={}",
                        item.id,
                        item.action,
                        item.target.label()
                    );
                } else {
                    let report = vela_runtime::remove_memory_entry(bootstrap, target, old_text)?;
                    println!(
                        "memory {}: target={} entries={} chars={}/{}",
                        report.action,
                        report.target.label(),
                        report.entry_count,
                        report.char_count,
                        report.char_limit
                    );
                }
            } else {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                let view = vela_runtime::view_memory(bootstrap, target)?;
                println!(
                    "{} [{} entries, {}/{} chars]",
                    view.target.label(),
                    view.entries.len(),
                    view.char_count,
                    view.char_limit
                );
                for (idx, entry) in view.entries.iter().enumerate() {
                    println!("{}. {}", idx + 1, entry);
                }
            }
        }
        Some(Commands::Skills(args)) => {
            if args.pending {
                let pending = vela_runtime::list_pending_skills(bootstrap)?;
                println!("pending skill writes [{}]:", pending.len());
                for item in pending {
                    println!("- {} :: action={} name={}", item.id, item.action, item.name);
                }
            } else if let Some(id) = args.show.as_deref() {
                let item = vela_runtime::get_pending_skill(bootstrap, id)?;
                println!("{}", serde_json::to_string_pretty(&item)?);
            } else if let Some(id) = args.approve.as_deref() {
                let report = vela_runtime::approve_pending_skill(bootstrap, id)?;
                println!(
                    "skill approve: {} {} ({})",
                    report.action,
                    report.name,
                    report.skill_md_path.display()
                );
            } else if let Some(id) = args.reject.as_deref() {
                vela_runtime::reject_pending_skill(bootstrap, id)?;
                println!("skill reject: {}", id);
            } else if let Some(name) = args.create.as_deref() {
                if args.stage {
                    let item = vela_runtime::stage_create_skill(
                        bootstrap,
                        name,
                        args.description.as_deref(),
                        args.body.as_deref(),
                    )?;
                    println!(
                        "skill staged: {} action={} name={}",
                        item.id, item.action, item.name
                    );
                } else {
                    let report = vela_runtime::create_skill(
                        bootstrap,
                        name,
                        args.description.as_deref(),
                        args.body.as_deref(),
                    )?;
                    println!(
                        "skill {}: {} ({})",
                        report.action,
                        report.name,
                        report.skill_md_path.display()
                    );
                }
            } else if let Some(name) = args.write.as_deref() {
                if args.stage {
                    let item = vela_runtime::stage_write_skill(
                        bootstrap,
                        name,
                        args.description.as_deref(),
                        args.body.as_deref(),
                    )?;
                    println!(
                        "skill staged: {} action={} name={}",
                        item.id, item.action, item.name
                    );
                } else {
                    let report = vela_runtime::write_skill(
                        bootstrap,
                        name,
                        args.description.as_deref(),
                        args.body.as_deref(),
                    )?;
                    println!(
                        "skill {}: {} ({})",
                        report.action,
                        report.name,
                        report.skill_md_path.display()
                    );
                }
            } else if let Some(name) = args.delete.as_deref() {
                if args.stage {
                    let item = vela_runtime::stage_delete_skill(bootstrap, name)?;
                    println!(
                        "skill staged: {} action={} name={}",
                        item.id, item.action, item.name
                    );
                } else {
                    let report = vela_runtime::delete_skill(bootstrap, name)?;
                    println!(
                        "skill {}: {} ({})",
                        report.action,
                        report.name,
                        report.skill_md_path.display()
                    );
                }
            } else if let Some(name) = args.view.as_deref() {
                let skill = vela_runtime::view_skill(bootstrap, name)?;
                println!("skill: {} ({})", skill.name, skill.skill_md_path.display());
                println!("---");
                println!("{}", skill.content);
            } else {
                let skills = vela_runtime::list_skills(bootstrap)?;
                println!("skills [{}]:", skills.len());
                for skill in skills {
                    println!(
                        "- {} :: {}{}",
                        skill.name,
                        skill.skill_md_path.display(),
                        skill
                            .description
                            .as_deref()
                            .map(|d| format!(" :: {}", d))
                            .unwrap_or_default()
                    );
                }
            }
        }
        Some(Commands::Review(args)) => {
            if args.auto {
                match vela_runtime::emit_review_signals_from_latest_session(bootstrap, args.limit)?
                {
                    Some(signal_report) => {
                        println!(
                            "review signals: session={} title={} emitted={} skipped={}",
                            signal_report.session_id,
                            signal_report.session_title,
                            signal_report.signals.len(),
                            signal_report.skipped
                        );
                        for signal in &signal_report.signals {
                            println!("- {} :: {}", signal.event_type, signal.payload_json);
                        }
                    }
                    None => println!("review signals: no session available"),
                }
                match vela_runtime::generate_review_candidates_from_latest_session(
                    bootstrap, args.limit,
                )? {
                    Some(report) => {
                        println!(
                            "review suggestions: session={} title={} created={} skipped={}",
                            report.session_id,
                            report.session_title,
                            report.candidate_ids.len(),
                            report.skipped
                        );
                        for id in report.candidate_ids {
                            println!("- {}", id);
                        }
                    }
                    None => println!("review suggestions: no session available"),
                }
            } else if args.emit_signals {
                match vela_runtime::emit_review_signals_from_latest_session(bootstrap, args.limit)?
                {
                    Some(report) => {
                        println!(
                            "review signals: session={} title={} emitted={} skipped={}",
                            report.session_id,
                            report.session_title,
                            report.signals.len(),
                            report.skipped
                        );
                        for signal in report.signals {
                            println!("- {} :: {}", signal.event_type, signal.payload_json);
                        }
                    }
                    None => println!("review signals: no session available"),
                }
            } else if args.suggest {
                match vela_runtime::generate_review_candidates_from_latest_session(
                    bootstrap, args.limit,
                )? {
                    Some(report) => {
                        println!(
                            "review suggestions: session={} title={} created={} skipped={}",
                            report.session_id,
                            report.session_title,
                            report.candidate_ids.len(),
                            report.skipped
                        );
                        for id in report.candidate_ids {
                            println!("- {}", id);
                        }
                    }
                    None => println!("review suggestions: no session available"),
                }
            } else if let Some(id) = args.show.as_deref() {
                let candidate = vela_runtime::get_review_candidate(bootstrap, id)?;
                println!("{}", serde_json::to_string_pretty(&candidate)?);
            } else if let Some(id) = args.promote.as_deref() {
                let report = vela_runtime::promote_review_candidate(bootstrap, id)?;
                println!(
                    "review promoted: candidate={} kind={} pending={}",
                    report.candidate_id,
                    report.kind.label(),
                    report.pending_id
                );
            } else if let Some(id) = args.reject.as_deref() {
                vela_runtime::reject_review_candidate(bootstrap, id)?;
                println!("review reject: {}", id);
            } else if let Some(content) = args.memory_add.as_deref() {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                let candidate = vela_runtime::stage_memory_review_candidate(
                    bootstrap,
                    target,
                    "add",
                    None,
                    Some(content),
                    args.reason
                        .as_deref()
                        .unwrap_or("Background review suggested new durable memory."),
                    args.source.as_deref(),
                )?;
                println!(
                    "review staged: {} kind={} source={}",
                    candidate.id,
                    candidate.kind.label(),
                    candidate.source
                );
            } else if let Some(content) = args.memory_replace.as_deref() {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                let old_text = args.match_text.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("--memory-replace requires --match <substring>")
                })?;
                let candidate = vela_runtime::stage_memory_review_candidate(
                    bootstrap,
                    target,
                    "replace",
                    Some(old_text),
                    Some(content),
                    args.reason
                        .as_deref()
                        .unwrap_or("Background review suggested refining durable memory."),
                    args.source.as_deref(),
                )?;
                println!(
                    "review staged: {} kind={} source={}",
                    candidate.id,
                    candidate.kind.label(),
                    candidate.source
                );
            } else if let Some(old_text) = args.memory_remove.as_deref() {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                let candidate = vela_runtime::stage_memory_review_candidate(
                    bootstrap,
                    target,
                    "remove",
                    Some(old_text),
                    None,
                    args.reason
                        .as_deref()
                        .unwrap_or("Background review suggested removing stale durable memory."),
                    args.source.as_deref(),
                )?;
                println!(
                    "review staged: {} kind={} source={}",
                    candidate.id,
                    candidate.kind.label(),
                    candidate.source
                );
            } else if let Some(name) = args.skill_create.as_deref() {
                let candidate = vela_runtime::stage_skill_review_candidate(
                    bootstrap,
                    "create",
                    name,
                    args.description.as_deref(),
                    args.body.as_deref(),
                    args.reason
                        .as_deref()
                        .unwrap_or("Background review suggested a new procedural memory skill."),
                    args.source.as_deref(),
                )?;
                println!(
                    "review staged: {} kind={} source={}",
                    candidate.id,
                    candidate.kind.label(),
                    candidate.source
                );
            } else if let Some(name) = args.skill_write.as_deref() {
                let candidate = vela_runtime::stage_skill_review_candidate(
                    bootstrap,
                    "write",
                    name,
                    args.description.as_deref(),
                    args.body.as_deref(),
                    args.reason.as_deref().unwrap_or(
                        "Background review suggested revising a procedural memory skill.",
                    ),
                    args.source.as_deref(),
                )?;
                println!(
                    "review staged: {} kind={} source={}",
                    candidate.id,
                    candidate.kind.label(),
                    candidate.source
                );
            } else if let Some(name) = args.skill_delete.as_deref() {
                let candidate = vela_runtime::stage_skill_review_candidate(
                    bootstrap,
                    "delete",
                    name,
                    None,
                    None,
                    args.reason.as_deref().unwrap_or(
                        "Background review suggested removing a stale procedural memory skill.",
                    ),
                    args.source.as_deref(),
                )?;
                println!(
                    "review staged: {} kind={} source={}",
                    candidate.id,
                    candidate.kind.label(),
                    candidate.source
                );
            } else {
                let candidates = vela_runtime::list_review_candidates(bootstrap)?;
                println!("review candidates [{}]:", candidates.len());
                for candidate in candidates {
                    println!(
                        "- {} :: kind={} source={} reason={}",
                        candidate.id,
                        candidate.kind.label(),
                        candidate.source,
                        candidate.reason
                    );
                }
            }
        }
        Some(Commands::Plan) => println!("docs/vela-rust-agentic-os-plan.md"),
        Some(Commands::Gateway(args)) => {
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
            }
            if !args.setup && !args.start {
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
        }
        Some(Commands::Sessions(args)) => {
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
        }
        Some(Commands::Cron(args)) => {
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
                        "- {} :: schedule={} source={} status={} next_run_at={} run_count={} recovery_count={} outcome={:?} last_error={:?} task={}",
                        job.id, job.schedule, job.source, job.status, job.next_run_at, job.run_count, job.recovery_count, job.last_outcome, job.last_error, job.task
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
                )?;
                println!(
                    "scheduled job added: {} schedule={} source={} status={} next_run_at={} task={}",
                    job.id, job.schedule, job.source, job.status, job.next_run_at, job.task
                );
            } else if let Some(id) = args.show.as_deref() {
                let job = vela_runtime::get_scheduled_job(bootstrap, id)?;
                println!(
                    "scheduled job: {} schedule={} source={} status={} created_at={} next_run_at={} run_count={} recovery_count={} outcome={:?} last_error={:?} task={}",
                    job.id, job.schedule, job.source, job.status, job.created_at, job.next_run_at, job.run_count, job.recovery_count, job.last_outcome, job.last_error, job.task
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
        }
        Some(Commands::Logs(args)) => println!(
            "logs placeholder: follow={} since={:?}",
            args.follow, args.since
        ),
        Some(Commands::Dashboard(args)) => println!(
            "dashboard placeholder: stop={} status={}",
            args.stop, args.status
        ),
        Some(other) => println!("placeholder command: {:?}", other),
    }

    Ok(())
}
