use super::*;

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

/// Inspects one persisted session with branch-aware title selection details.
pub fn inspect_session_selection(
    bootstrap: &BootstrapReport,
    target: &str,
    limit: usize,
) -> Result<SessionInspectionSelection> {
    vela_state::inspect_session_selection(&bootstrap.persistence.state_db_path, target, limit)
}

/// Lists recent persisted sessions with branch-aware depth.
pub fn list_sessions(bootstrap: &BootstrapReport, limit: usize) -> Result<Vec<SessionBranchNode>> {
    vela_state::list_sessions(&bootstrap.persistence.state_db_path, limit)
}

/// Browses session trees grouped by root session.
pub fn browse_session_branches(
    bootstrap: &BootstrapReport,
    root_limit: usize,
    descendant_limit: usize,
) -> Result<Vec<SessionBrowseTree>> {
    vela_state::browse_session_branches(
        &bootstrap.persistence.state_db_path,
        root_limit,
        descendant_limit,
    )
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
    if let Err(error) = append_review_event(
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
    ) {
        tracing::warn!(candidate_id=%candidate.id, error=%error, "failed to append review_candidate_created event");
    }
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
    if let Err(error) = append_review_event(
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
    ) {
        tracing::warn!(candidate_id=%candidate.id, error=%error, "failed to append review_candidate_created event");
    }
    Ok(candidate)
}

/// Promotes a review candidate into the appropriate pending approval queue.
pub fn promote_review_candidate(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_review::PromotionReport> {
    let candidate = vela_review::get_candidate(&bootstrap.vela_home, id)?;
    let report = vela_review::promote_candidate(&bootstrap.vela_home, id)?;
    if let Err(error) = append_review_event(
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
    ) {
        tracing::warn!(candidate_id=%report.candidate_id, error=%error, "failed to append review_candidate_promoted event");
    }
    Ok(report)
}

/// Rejects a queued review candidate.
pub fn reject_review_candidate(bootstrap: &BootstrapReport, id: &str) -> Result<()> {
    let candidate = vela_review::reject_candidate(&bootstrap.vela_home, id)?;
    if let Err(error) = append_review_event(
        bootstrap,
        candidate.origin_session_id.as_deref(),
        "review_candidate_rejected",
        json!({
            "candidate_id": id,
            "origin_session_id": candidate.origin_session_id.clone(),
            "origin_session_title": candidate.origin_session_title.clone(),
        })
        .to_string(),
    ) {
        tracing::warn!(candidate_id=%id, error=%error, "failed to append review_candidate_rejected event");
    }
    Ok(())
}
