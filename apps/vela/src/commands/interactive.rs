use crate::cli::{print_extension_record, ChatArgs, Cli, ExtensionsArgs};
use anyhow::Result;

pub(crate) fn run_chat(bootstrap: &vela_runtime::BootstrapReport, args: &ChatArgs) -> Result<()> {
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
    print_chat_report(&report);
    if args.checkpoints {
        println!(
            "\ncheckpoints: signals={} candidates={}",
            report.emitted_signal_count, report.generated_candidate_count
        );
    }
    Ok(())
}

pub(crate) fn run_default_chat(bootstrap: &vela_runtime::BootstrapReport, cli: &Cli) -> Result<()> {
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
    print_chat_report(&report);
    Ok(())
}

fn print_chat_report(report: &vela_runtime::ChatTurnReport) {
    println!(
        "runtime session: action={} id={} title={} mode={}",
        report.session.action.label(),
        report.session.session_id,
        report.session.title,
        report.session.interaction_mode.label(),
    );
    if let Some(mode) = report.session.continue_resolution.as_deref() {
        println!(
            "continue resolution: mode={} target={:?} anchor_id={:?} anchor_title={:?} resolved_id={}",
            mode,
            report.session.continue_target,
            report.session.continue_anchor_session_id,
            report.session.continue_anchor_title,
            report.session.session_id,
        );
    }
    if let Some(response) = report.response.as_deref() {
        println!("\n{}", response);
    }
    println!(
        "\nlifecycle: turn={} phases={} last={}",
        report.turn_id, report.lifecycle_phase_count, report.final_phase
    );
}

pub(crate) fn run_status(bootstrap: &vela_runtime::BootstrapReport) -> Result<()> {
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
    Ok(())
}

pub(crate) fn run_extensions(
    bootstrap: &vela_runtime::BootstrapReport,
    args: &ExtensionsArgs,
) -> Result<()> {
    if args.reload {
        let report = vela_runtime::reload_extensions(bootstrap)?;
        println!("extensions reloaded: {}", report.summary_line());
        println!(
            "session preserved: {} before={:?} after={:?}",
            report.preserved_session,
            report.session_before.as_ref().map(|item| item.id.as_str()),
            report.session_after.as_ref().map(|item| item.id.as_str()),
        );
        if report.restart_required_fields.is_empty() {
            println!("restart required: none");
        } else {
            println!(
                "restart required: {}",
                report.restart_required_fields.join(", ")
            );
        }
        for entry in &report.extensions.entries {
            print_extension_record(entry);
        }
    } else {
        println!("{}", bootstrap.extensions.summary_line());
        for entry in &bootstrap.extensions.entries {
            print_extension_record(entry);
        }
    }
    Ok(())
}
