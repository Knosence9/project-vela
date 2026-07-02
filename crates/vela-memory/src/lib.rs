use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Defines the maximum prompt-facing character budget for durable memory entries.
pub const MEMORY_CHAR_LIMIT: usize = 2_200;
/// Defines the maximum prompt-facing character budget for user-profile entries.
pub const USER_CHAR_LIMIT: usize = 1_375;
const LEGACY_ENTRY_SEPARATOR: &str = "§";
const ENTRY_GAP: &str = "\n\n";
const STORAGE_FORMAT_MARKER: &str = "<!-- vela-memory-format: v2 -->";

mod surface;

#[cfg(test)]
mod tests;

pub use surface::*;

fn replace_exact_memory_entry(
    vela_home: &Path,
    target: MemoryTarget,
    matched_entry: &str,
    content: &str,
) -> Result<MemoryMutationReport> {
    let mut entries = load_entries(vela_home, target)?;
    let idx = exact_match_index(&entries, matched_entry)?;
    entries[idx] = sanitize_entry(content)?;
    save_entries(vela_home, target, &entries)?;
    report(target, "replace", &entries)
}

fn remove_exact_memory_entry(
    vela_home: &Path,
    target: MemoryTarget,
    matched_entry: &str,
) -> Result<MemoryMutationReport> {
    let mut entries = load_entries(vela_home, target)?;
    let idx = exact_match_index(&entries, matched_entry)?;
    entries.remove(idx);
    save_entries(vela_home, target, &entries)?;
    report(target, "remove", &entries)
}

/// Stages remove memory entry for explicit approval.
pub fn stage_remove_memory_entry(
    vela_home: &Path,
    target: MemoryTarget,
    old_text: &str,
) -> Result<PendingMemoryWrite> {
    if old_text.trim().is_empty() {
        bail!("match text cannot be empty");
    }
    let entries = load_entries(vela_home, target)?;
    let idx = unique_match_index(&entries, old_text)?;
    stage_write(
        vela_home,
        PendingMemoryWrite {
            id: new_pending_id("mem"),
            target,
            action: "remove".to_string(),
            old_text: Some(old_text.trim().to_string()),
            matched_entry: Some(entries[idx].clone()),
            new_text: None,
            created_at: unix_timestamp(),
        },
    )
}

/// Lists pending available in this subsystem.
pub fn list_pending(vela_home: &Path) -> Result<Vec<PendingMemoryWrite>> {
    let dir = pending_dir(vela_home);
    fs::create_dir_all(&dir)?;
    let mut items = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let pending: PendingMemoryWrite = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        items.push(pending);
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(items)
}

/// Retrieves pending by identifier.
pub fn get_pending(vela_home: &Path, id: &str) -> Result<PendingMemoryWrite> {
    let id = validate_pending_id(id)?;
    let path = pending_dir(vela_home).join(format!("{id}.json"));
    let text = fs::read_to_string(&path)
        .with_context(|| format!("pending memory write {:?} not found", id))?;
    Ok(serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?)
}

/// Rejects pending without applying it.
pub fn reject_pending(vela_home: &Path, id: &str) -> Result<()> {
    let id = validate_pending_id(id)?;
    let path = pending_dir(vela_home).join(format!("{id}.json"));
    fs::remove_file(&path).with_context(|| format!("pending memory write {:?} not found", id))?;
    Ok(())
}

/// Approves pending and applies it durably.
pub fn approve_pending(vela_home: &Path, id: &str) -> Result<MemoryMutationReport> {
    let pending = get_pending(vela_home, id)?;
    let result = match pending.action.as_str() {
        "add" => add_memory_entry(
            vela_home,
            pending.target,
            pending
                .new_text
                .as_deref()
                .ok_or_else(|| anyhow!("pending add missing new_text"))?,
        )?,
        "replace" => replace_exact_memory_entry(
            vela_home,
            pending.target,
            pending
                .matched_entry
                .as_deref()
                .ok_or_else(|| anyhow!("pending replace missing matched_entry"))?,
            pending
                .new_text
                .as_deref()
                .ok_or_else(|| anyhow!("pending replace missing new_text"))?,
        )
        .with_context(|| format!("pending replace {} became stale or conflicted", pending.id))?,
        "remove" => remove_exact_memory_entry(
            vela_home,
            pending.target,
            pending
                .matched_entry
                .as_deref()
                .ok_or_else(|| anyhow!("pending remove missing matched_entry"))?,
        )
        .with_context(|| format!("pending remove {} became stale or conflicted", pending.id))?,
        other => bail!("unknown pending memory action {other:?}"),
    };
    reject_pending(vela_home, id)?;
    Ok(result)
}

/// Renders prompt snapshot for prompt or display use.
pub fn render_prompt_snapshot(vela_home: &Path) -> Result<String> {
    let memory = load_snapshot(vela_home, MemoryTarget::Memory)?;
    let user = load_snapshot(vela_home, MemoryTarget::User)?;
    Ok(format!(
        "{} [{}% — {}/{} chars]\n{}\n\n{} [{}% — {}/{} chars]\n{}",
        memory.target.heading(),
        percent(memory.char_count, memory.char_limit),
        memory.char_count,
        memory.char_limit,
        memory.content.trim(),
        user.target.heading(),
        percent(user.char_count, user.char_limit),
        user.char_count,
        user.char_limit,
        user.content.trim(),
    ))
}

fn stage_write(vela_home: &Path, pending: PendingMemoryWrite) -> Result<PendingMemoryWrite> {
    let dir = pending_dir(vela_home);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", pending.id));
    fs::write(&path, serde_json::to_string_pretty(&pending)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(pending)
}

fn report(
    target: MemoryTarget,
    action: &'static str,
    entries: &[String],
) -> Result<MemoryMutationReport> {
    let rendered = render_entries(entries);
    Ok(MemoryMutationReport {
        target,
        action,
        char_count: rendered.chars().count(),
        char_limit: target.limit(),
        entry_count: entries.len(),
    })
}

fn pending_dir(vela_home: &Path) -> PathBuf {
    vela_home.join("pending").join("memory")
}

fn target_path(vela_home: &Path, target: MemoryTarget) -> PathBuf {
    vela_home.join("memories").join(target.filename())
}

fn load_entries(vela_home: &Path, target: MemoryTarget) -> Result<Vec<String>> {
    let path = target_path(vela_home, target);
    ensure_file(&path)?;
    migrate_legacy_file(&path)?;
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(parse_entries(&content))
}

fn save_entries(vela_home: &Path, target: MemoryTarget, entries: &[String]) -> Result<()> {
    let rendered = render_entries(entries);
    let char_count = rendered.chars().count();
    if char_count > target.limit() {
        bail!(
            "{} write would exceed limit: {} > {} chars",
            target.label(),
            char_count,
            target.limit()
        );
    }
    let path = target_path(vela_home, target);
    fs::write(&path, render_storage(entries))
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn parse_entries(content: &str) -> Vec<String> {
    let trimmed = strip_storage_marker(content).trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if is_legacy_separator_format(content) {
        return trimmed
            .split(LEGACY_ENTRY_SEPARATOR)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(ToOwned::to_owned)
            .collect();
    }

    trimmed
        .split(ENTRY_GAP)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn render_entries(entries: &[String]) -> String {
    entries.join(ENTRY_GAP)
}

fn render_storage(entries: &[String]) -> String {
    if entries.is_empty() {
        format!("{STORAGE_FORMAT_MARKER}\n")
    } else {
        format!(
            "{STORAGE_FORMAT_MARKER}{ENTRY_GAP}{}",
            render_entries(entries)
        )
    }
}

fn sanitize_entry(content: &str) -> Result<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        bail!("memory entry cannot be empty");
    }
    Ok(trimmed.to_string())
}

fn exact_match_index(entries: &[String], expected: &str) -> Result<usize> {
    let matches: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| (entry == expected).then_some(idx))
        .collect();
    match matches.as_slice() {
        [idx] => Ok(*idx),
        [] => Err(anyhow!(
            "staged memory entry no longer exists exactly as reviewed"
        )),
        _ => Err(anyhow!(
            "staged memory entry became ambiguous; resolve manually"
        )),
    }
}

fn unique_match_index(entries: &[String], needle: &str) -> Result<usize> {
    let needle = needle.trim();
    if needle.is_empty() {
        bail!("match text cannot be empty");
    }
    let matches: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| entry.contains(needle).then_some(idx))
        .collect();
    match matches.as_slice() {
        [idx] => Ok(*idx),
        [] => Err(anyhow!("no memory entry matched {needle:?}")),
        _ => Err(anyhow!(
            "memory match {needle:?} was ambiguous; use a more specific substring"
        )),
    }
}

fn migrate_legacy_file(path: &Path) -> Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    if !is_legacy_separator_format(&content) {
        return Ok(());
    }
    let migrated = render_storage(&parse_entries(&content));
    fs::write(path, migrated).with_context(|| format!("failed to migrate {}", path.display()))?;
    Ok(())
}

fn is_legacy_separator_format(content: &str) -> bool {
    let trimmed = content.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with(STORAGE_FORMAT_MARKER)
        && !trimmed.contains('\n')
        && trimmed.contains(LEGACY_ENTRY_SEPARATOR)
}

fn strip_storage_marker(content: &str) -> &str {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix(STORAGE_FORMAT_MARKER) {
        rest.trim_start_matches(['\r', '\n', ' '])
    } else {
        content
    }
}

fn validate_pending_id(id: &str) -> Result<&str> {
    let id = id.trim();
    if id.is_empty() || id == "." || id == ".." || id.contains('/') || id.contains('\\') {
        bail!("invalid pending memory id");
    }
    Ok(id)
}

fn ensure_file(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::write(path, format!("{STORAGE_FORMAT_MARKER}\n"))
            .with_context(|| format!("failed to create {}", path.display()))?;
    }
    Ok(())
}

fn new_pending_id(prefix: &str) -> String {
    format!("{}-{}", prefix, unix_timestamp_nanos())
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn unix_timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn percent(count: usize, limit: usize) -> usize {
    if limit == 0 {
        0
    } else {
        ((count as f64 / limit as f64) * 100.0).round() as usize
    }
}
