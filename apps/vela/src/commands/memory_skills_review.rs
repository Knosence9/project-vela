use crate::cli::{MemoryArgs, ReviewArgs, SkillsArgs};
use anyhow::Result;

pub(crate) fn run_memory(
    bootstrap: &vela_runtime::BootstrapReport,
    args: &MemoryArgs,
) -> Result<()> {
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
            let item =
                vela_runtime::stage_replace_memory_entry(bootstrap, target, old_text, content)?;
            println!(
                "memory staged: {} action={} target={}",
                item.id,
                item.action,
                item.target.label()
            );
        } else {
            let report = vela_runtime::replace_memory_entry(bootstrap, target, old_text, content)?;
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
            let item = vela_runtime::stage_remove_memory_entry(bootstrap, target, old_text)?;
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
    Ok(())
}

pub(crate) fn run_skills(
    bootstrap: &vela_runtime::BootstrapReport,
    args: &SkillsArgs,
) -> Result<()> {
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
    Ok(())
}

pub(crate) fn run_review(
    bootstrap: &vela_runtime::BootstrapReport,
    args: &ReviewArgs,
) -> Result<()> {
    if args.auto {
        match vela_runtime::emit_review_signals_from_latest_session(bootstrap, args.limit)? {
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
        match vela_runtime::generate_review_candidates_from_latest_session(bootstrap, args.limit)? {
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
        match vela_runtime::emit_review_signals_from_latest_session(bootstrap, args.limit)? {
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
        match vela_runtime::generate_review_candidates_from_latest_session(bootstrap, args.limit)? {
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
        let old_text = args
            .match_text
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--memory-replace requires --match <substring>"))?;
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
            args.reason
                .as_deref()
                .unwrap_or("Background review suggested revising a procedural memory skill."),
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
            args.reason
                .as_deref()
                .unwrap_or("Background review suggested removing a stale procedural memory skill."),
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
    Ok(())
}
