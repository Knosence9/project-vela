use anyhow::Result;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "vela")]
#[command(about = "Rust-first agentic OS shell for Vela kernel work")]
struct Cli {
    #[arg(short = 'z', long = "oneshot")]
    oneshot: Option<String>,
    #[arg(short = 'm', long = "model")]
    model: Option<String>,
    #[arg(long = "provider")]
    provider: Option<String>,
    #[arg(short = 't', long = "toolsets")]
    toolsets: Option<String>,
    #[arg(short = 'r', long = "resume")]
    resume: Option<String>,
    #[arg(short = 's', long = "skills")]
    skills: Vec<String>,
    #[arg(short = 'c', long = "continue")]
    continue_last: Option<String>,
    #[arg(short = 'w', long = "worktree", default_value_t = false)]
    worktree: bool,
    #[arg(long = "accept-hooks", default_value_t = false)]
    accept_hooks: bool,
    #[arg(long = "yolo", default_value_t = false)]
    yolo: bool,
    #[arg(long = "pass-session-id", default_value_t = false)]
    pass_session_id: bool,
    #[arg(long = "ignore-user-config", default_value_t = false)]
    ignore_user_config: bool,
    #[arg(long = "ignore-rules", default_value_t = false)]
    ignore_rules: bool,
    #[arg(long = "safe-mode", default_value_t = false)]
    safe_mode: bool,
    #[arg(short = 'p', long = "profile")]
    profile: Option<String>,
    #[arg(long = "cli", default_value_t = false)]
    cli_mode: bool,
    #[arg(long = "tui", default_value_t = false)]
    tui_mode: bool,
    #[arg(long = "version", default_value_t = false)]
    version: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Default, Args)]
struct ChatArgs {
    #[arg(short = 'q', long = "query")]
    query: Option<String>,
    #[arg(long = "image")]
    image: Option<String>,
    #[arg(short = 'm', long = "model")]
    model: Option<String>,
    #[arg(short = 't', long = "toolsets")]
    toolsets: Option<String>,
    #[arg(short = 's', long = "skills")]
    skills: Vec<String>,
    #[arg(long = "provider")]
    provider: Option<String>,
    #[arg(short = 'v', long = "verbose", default_value_t = false)]
    verbose: bool,
    #[arg(short = 'r', long = "resume")]
    resume: Option<String>,
    #[arg(short = 'c', long = "continue")]
    continue_last: Option<String>,
    #[arg(short = 'w', long = "worktree", default_value_t = false)]
    worktree: bool,
    #[arg(long = "accept-hooks", default_value_t = false)]
    accept_hooks: bool,
    #[arg(long = "checkpoints", default_value_t = false)]
    checkpoints: bool,
    #[arg(long = "max-turns")]
    max_turns: Option<u32>,
    #[arg(long = "yolo", default_value_t = false)]
    yolo: bool,
    #[arg(long = "pass-session-id", default_value_t = false)]
    pass_session_id: bool,
}

#[derive(Debug, Default, Args)]
struct GatewayArgs {
    #[arg(long = "setup", default_value_t = false)]
    setup: bool,
    #[arg(long = "start", default_value_t = false)]
    start: bool,
}

#[derive(Debug, Default, Args)]
struct SessionsArgs {
    #[arg(long = "list", default_value_t = false)]
    list: bool,
    #[arg(long = "browse", default_value_t = false)]
    browse: bool,
    #[arg(long = "search")]
    search: Option<String>,
}

#[derive(Debug, Default, Args)]
struct LogsArgs {
    #[arg(short = 'f', long = "follow", default_value_t = false)]
    follow: bool,
    #[arg(long = "since")]
    since: Option<String>,
}

#[derive(Debug, Default, Args)]
struct DashboardArgs {
    #[arg(long = "stop", default_value_t = false)]
    stop: bool,
    #[arg(long = "status", default_value_t = false)]
    status: bool,
}

#[derive(Debug, Default, Args)]
struct SkillsArgs {
    #[arg(long = "list", default_value_t = false, group = "action")]
    list: bool,
    #[arg(long = "view", group = "action")]
    view: Option<String>,
    #[arg(long = "create", group = "action")]
    create: Option<String>,
    #[arg(long = "write", group = "action")]
    write: Option<String>,
    #[arg(long = "delete", group = "action")]
    delete: Option<String>,
    #[arg(long = "description")]
    description: Option<String>,
    #[arg(long = "body")]
    body: Option<String>,
    #[arg(long = "stage", default_value_t = false)]
    stage: bool,
    #[arg(long = "pending", default_value_t = false, group = "action")]
    pending: bool,
    #[arg(long = "approve", group = "action")]
    approve: Option<String>,
    #[arg(long = "reject", group = "action")]
    reject: Option<String>,
    #[arg(long = "show", group = "action")]
    show: Option<String>,
}

#[derive(Debug, Default, Args)]
struct ReviewArgs {
    #[arg(long = "list", default_value_t = false, group = "action")]
    list: bool,
    #[arg(long = "emit-signals", default_value_t = false, group = "action")]
    emit_signals: bool,
    #[arg(long = "suggest", default_value_t = false, group = "action")]
    suggest: bool,
    #[arg(long = "auto", default_value_t = false, group = "action")]
    auto: bool,
    #[arg(long = "limit", default_value_t = 20)]
    limit: usize,
    #[arg(long = "show", group = "action")]
    show: Option<String>,
    #[arg(long = "promote", group = "action")]
    promote: Option<String>,
    #[arg(long = "reject", group = "action")]
    reject: Option<String>,
    #[arg(long = "target", default_value = "memory")]
    target: String,
    #[arg(long = "memory-add", group = "action")]
    memory_add: Option<String>,
    #[arg(long = "memory-replace", group = "action")]
    memory_replace: Option<String>,
    #[arg(long = "memory-remove", group = "action")]
    memory_remove: Option<String>,
    #[arg(long = "match")]
    match_text: Option<String>,
    #[arg(long = "skill-create", group = "action")]
    skill_create: Option<String>,
    #[arg(long = "skill-write", group = "action")]
    skill_write: Option<String>,
    #[arg(long = "skill-delete", group = "action")]
    skill_delete: Option<String>,
    #[arg(long = "description")]
    description: Option<String>,
    #[arg(long = "body")]
    body: Option<String>,
    #[arg(long = "reason")]
    reason: Option<String>,
    #[arg(long = "source")]
    source: Option<String>,
}

#[derive(Debug, Default, Args)]
struct MemoryArgs {
    #[arg(long = "target", default_value = "memory")]
    target: String,
    #[arg(long = "view", default_value_t = false, group = "action")]
    view: bool,
    #[arg(long = "prompt-snapshot", default_value_t = false, group = "action")]
    prompt_snapshot: bool,
    #[arg(long = "add", group = "action")]
    add: Option<String>,
    #[arg(long = "replace", group = "action")]
    replace: Option<String>,
    #[arg(long = "match")]
    match_text: Option<String>,
    #[arg(long = "remove", group = "action")]
    remove: Option<String>,
    #[arg(long = "stage", default_value_t = false)]
    stage: bool,
    #[arg(long = "pending", default_value_t = false, group = "action")]
    pending: bool,
    #[arg(long = "approve", group = "action")]
    approve: Option<String>,
    #[arg(long = "reject", group = "action")]
    reject: Option<String>,
    #[arg(long = "show", group = "action")]
    show: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Chat(ChatArgs),
    Setup,
    Gateway(GatewayArgs),
    Sessions(SessionsArgs),
    Logs(LogsArgs),
    Model,
    Config,
    Skills(SkillsArgs),
    Tools,
    Memory(MemoryArgs),
    Review(ReviewArgs),
    Cron,
    Mcp,
    Status,
    Update,
    Dashboard(DashboardArgs),
    Auth,
    Pairing,
    Version,
    Plan,
}

fn main() -> Result<()> {
    let (argv, preparse_profile) = vela_runtime::preparse_profile_override(std::env::args())?;
    tracing_subscriber::fmt::init();
    let cli = Cli::parse_from(argv);

    if cli.version {
        println!("vela-rs 0.1.0-kernel");
        vela_runtime::bootstrap_banner();
        return Ok(());
    }

    let bootstrap = vela_runtime::initialize_bootstrap(
        preparse_profile.or_else(|| cli.profile.clone()),
        cli.ignore_user_config,
    )?;

    match cli.command {
        Some(Commands::Chat(args)) => {
            let report = vela_runtime::resolve_runtime_session(
                &bootstrap,
                &vela_runtime::SessionRequest {
                    command_name: "chat".to_string(),
                    query_present: args.query.is_some(),
                    query_text: args.query.clone(),
                    image_present: args.image.is_some(),
                    image_path: args.image.clone(),
                    resume: args.resume.clone(),
                    continue_last: args.continue_last.clone(),
                },
            )?;
            println!(
                "runtime session: action={} id={} title={} mode={}",
                report.action.label(),
                report.session_id,
                report.title,
                report.interaction_mode.label(),
            );
        }
        None => {
            let report = vela_runtime::resolve_runtime_session(
                &bootstrap,
                &vela_runtime::SessionRequest {
                    command_name: "chat".to_string(),
                    query_present: false,
                    query_text: None,
                    image_present: false,
                    image_path: None,
                    resume: cli.resume.clone(),
                    continue_last: cli.continue_last.clone(),
                },
            )?;
            println!(
                "runtime session: action={} id={} title={} mode={}",
                report.action.label(),
                report.session_id,
                report.title,
                report.interaction_mode.label(),
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
                "resolved config: display.interface={:?} hooks_auto_accept={:?} security.redact_secrets={:?} network.force_ipv4={:?}",
                bootstrap.resolved_config.display_interface,
                bootstrap.resolved_config.hooks_auto_accept,
                bootstrap.resolved_config.security_redact_secrets,
                bootstrap.resolved_config.network_force_ipv4,
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
            match vela_runtime::current_session_summary(&bootstrap)? {
                Some(summary) => println!(
                    "active session: id={} title={} messages={} events={}",
                    summary.id, summary.title, summary.message_count, summary.event_count
                ),
                None => println!("active session: none"),
            }
        }
        Some(Commands::Memory(args)) => {
            if args.prompt_snapshot {
                println!("{}", vela_runtime::render_memory_snapshot(&bootstrap)?);
            } else {
                if args.pending {
                    let pending = vela_runtime::list_pending_memory(&bootstrap)?;
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
                    let item = vela_runtime::get_pending_memory(&bootstrap, id)?;
                    println!("{}", serde_json::to_string_pretty(&item)?);
                } else if let Some(id) = args.approve.as_deref() {
                    let report = vela_runtime::approve_pending_memory(&bootstrap, id)?;
                    println!("memory approve: target={} entries={} chars={}/{}", report.target.label(), report.entry_count, report.char_count, report.char_limit);
                } else if let Some(id) = args.reject.as_deref() {
                    vela_runtime::reject_pending_memory(&bootstrap, id)?;
                    println!("memory reject: {}", id);
                } else if let Some(content) = args.add.as_deref() {
                    let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                    if args.stage {
                        let item = vela_runtime::stage_add_memory_entry(&bootstrap, target, content)?;
                        println!("memory staged: {} action={} target={}", item.id, item.action, item.target.label());
                    } else {
                        let report = vela_runtime::add_memory_entry(&bootstrap, target, content)?;
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
                        let item = vela_runtime::stage_replace_memory_entry(&bootstrap, target, old_text, content)?;
                        println!("memory staged: {} action={} target={}", item.id, item.action, item.target.label());
                    } else {
                        let report = vela_runtime::replace_memory_entry(&bootstrap, target, old_text, content)?;
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
                        let item = vela_runtime::stage_remove_memory_entry(&bootstrap, target, old_text)?;
                        println!("memory staged: {} action={} target={}", item.id, item.action, item.target.label());
                    } else {
                        let report = vela_runtime::remove_memory_entry(&bootstrap, target, old_text)?;
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
                    let view = vela_runtime::view_memory(&bootstrap, target)?;
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
        }
        Some(Commands::Skills(args)) => {
            if args.pending {
                let pending = vela_runtime::list_pending_skills(&bootstrap)?;
                println!("pending skill writes [{}]:", pending.len());
                for item in pending {
                    println!("- {} :: action={} name={}", item.id, item.action, item.name);
                }
            } else if let Some(id) = args.show.as_deref() {
                let item = vela_runtime::get_pending_skill(&bootstrap, id)?;
                println!("{}", serde_json::to_string_pretty(&item)?);
            } else if let Some(id) = args.approve.as_deref() {
                let report = vela_runtime::approve_pending_skill(&bootstrap, id)?;
                println!("skill approve: {} {} ({})", report.action, report.name, report.skill_md_path.display());
            } else if let Some(id) = args.reject.as_deref() {
                vela_runtime::reject_pending_skill(&bootstrap, id)?;
                println!("skill reject: {}", id);
            } else if let Some(name) = args.create.as_deref() {
                if args.stage {
                    let item = vela_runtime::stage_create_skill(&bootstrap, name, args.description.as_deref(), args.body.as_deref())?;
                    println!("skill staged: {} action={} name={}", item.id, item.action, item.name);
                } else {
                    let report = vela_runtime::create_skill(
                        &bootstrap,
                        name,
                        args.description.as_deref(),
                        args.body.as_deref(),
                    )?;
                    println!("skill {}: {} ({})", report.action, report.name, report.skill_md_path.display());
                }
            } else if let Some(name) = args.write.as_deref() {
                if args.stage {
                    let item = vela_runtime::stage_write_skill(&bootstrap, name, args.description.as_deref(), args.body.as_deref())?;
                    println!("skill staged: {} action={} name={}", item.id, item.action, item.name);
                } else {
                    let report = vela_runtime::write_skill(
                        &bootstrap,
                        name,
                        args.description.as_deref(),
                        args.body.as_deref(),
                    )?;
                    println!("skill {}: {} ({})", report.action, report.name, report.skill_md_path.display());
                }
            } else if let Some(name) = args.delete.as_deref() {
                if args.stage {
                    let item = vela_runtime::stage_delete_skill(&bootstrap, name)?;
                    println!("skill staged: {} action={} name={}", item.id, item.action, item.name);
                } else {
                    let report = vela_runtime::delete_skill(&bootstrap, name)?;
                    println!("skill {}: {} ({})", report.action, report.name, report.skill_md_path.display());
                }
            } else if let Some(name) = args.view.as_deref() {
                let skill = vela_runtime::view_skill(&bootstrap, name)?;
                println!("skill: {} ({})", skill.name, skill.skill_md_path.display());
                println!("---");
                println!("{}", skill.content);
            } else {
                let skills = vela_runtime::list_skills(&bootstrap)?;
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
                match vela_runtime::emit_review_signals_from_latest_session(&bootstrap, args.limit)? {
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
                match vela_runtime::generate_review_candidates_from_latest_session(&bootstrap, args.limit)? {
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
                match vela_runtime::emit_review_signals_from_latest_session(&bootstrap, args.limit)? {
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
                match vela_runtime::generate_review_candidates_from_latest_session(&bootstrap, args.limit)? {
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
                let candidate = vela_runtime::get_review_candidate(&bootstrap, id)?;
                println!("{}", serde_json::to_string_pretty(&candidate)?);
            } else if let Some(id) = args.promote.as_deref() {
                let report = vela_runtime::promote_review_candidate(&bootstrap, id)?;
                println!(
                    "review promoted: candidate={} kind={} pending={}",
                    report.candidate_id,
                    report.kind.label(),
                    report.pending_id
                );
            } else if let Some(id) = args.reject.as_deref() {
                vela_runtime::reject_review_candidate(&bootstrap, id)?;
                println!("review reject: {}", id);
            } else if let Some(content) = args.memory_add.as_deref() {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                let candidate = vela_runtime::stage_memory_review_candidate(
                    &bootstrap,
                    target,
                    "add",
                    None,
                    Some(content),
                    args.reason.as_deref().unwrap_or("Background review suggested new durable memory."),
                    args.source.as_deref(),
                )?;
                println!("review staged: {} kind={} source={}", candidate.id, candidate.kind.label(), candidate.source);
            } else if let Some(content) = args.memory_replace.as_deref() {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                let old_text = args
                    .match_text
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--memory-replace requires --match <substring>"))?;
                let candidate = vela_runtime::stage_memory_review_candidate(
                    &bootstrap,
                    target,
                    "replace",
                    Some(old_text),
                    Some(content),
                    args.reason.as_deref().unwrap_or("Background review suggested refining durable memory."),
                    args.source.as_deref(),
                )?;
                println!("review staged: {} kind={} source={}", candidate.id, candidate.kind.label(), candidate.source);
            } else if let Some(old_text) = args.memory_remove.as_deref() {
                let target = vela_runtime::MemoryTarget::parse(&args.target)?;
                let candidate = vela_runtime::stage_memory_review_candidate(
                    &bootstrap,
                    target,
                    "remove",
                    Some(old_text),
                    None,
                    args.reason.as_deref().unwrap_or("Background review suggested removing stale durable memory."),
                    args.source.as_deref(),
                )?;
                println!("review staged: {} kind={} source={}", candidate.id, candidate.kind.label(), candidate.source);
            } else if let Some(name) = args.skill_create.as_deref() {
                let candidate = vela_runtime::stage_skill_review_candidate(
                    &bootstrap,
                    "create",
                    name,
                    args.description.as_deref(),
                    args.body.as_deref(),
                    args.reason.as_deref().unwrap_or("Background review suggested a new procedural memory skill."),
                    args.source.as_deref(),
                )?;
                println!("review staged: {} kind={} source={}", candidate.id, candidate.kind.label(), candidate.source);
            } else if let Some(name) = args.skill_write.as_deref() {
                let candidate = vela_runtime::stage_skill_review_candidate(
                    &bootstrap,
                    "write",
                    name,
                    args.description.as_deref(),
                    args.body.as_deref(),
                    args.reason.as_deref().unwrap_or("Background review suggested revising a procedural memory skill."),
                    args.source.as_deref(),
                )?;
                println!("review staged: {} kind={} source={}", candidate.id, candidate.kind.label(), candidate.source);
            } else if let Some(name) = args.skill_delete.as_deref() {
                let candidate = vela_runtime::stage_skill_review_candidate(
                    &bootstrap,
                    "delete",
                    name,
                    None,
                    None,
                    args.reason.as_deref().unwrap_or("Background review suggested removing a stale procedural memory skill."),
                    args.source.as_deref(),
                )?;
                println!("review staged: {} kind={} source={}", candidate.id, candidate.kind.label(), candidate.source);
            } else {
                let candidates = vela_runtime::list_review_candidates(&bootstrap)?;
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
            if args.setup {
                let report = vela_runtime::setup_gateway(&bootstrap)?;
                println!(
                    "gateway setup: dir={} config={} existed_before={} inbox={} outbox={}",
                    report.gateway_dir.display(),
                    report.config_path.display(),
                    report.config_existed_before,
                    report.inbox_dir.display(),
                    report.outbox_dir.display(),
                );
            }
            if args.start {
                let report = vela_runtime::start_gateway(&bootstrap)?;
                println!(
                    "gateway started: session={} action={} title={} config={}",
                    report.session.session_id,
                    report.session.action.label(),
                    report.session.title,
                    report.setup.config_path.display(),
                );
            }
            if !args.setup && !args.start {
                let report = vela_runtime::setup_gateway(&bootstrap)?;
                match vela_runtime::current_command_session_summary(&bootstrap, "gateway")? {
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
            if let Some(query) = args.search.as_deref() {
                let hits = vela_runtime::search_session_history(&bootstrap, query, 10)?;
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
                println!("sessions placeholder: list={} browse={}", args.list, args.browse);
            }
        }
        Some(Commands::Logs(args)) => println!("logs placeholder: follow={} since={:?}", args.follow, args.since),
        Some(Commands::Dashboard(args)) => println!("dashboard placeholder: stop={} status={}", args.stop, args.status),
        Some(other) => println!("placeholder command: {:?}", other),
    }

    vela_runtime::bootstrap_banner();
    Ok(())
}
