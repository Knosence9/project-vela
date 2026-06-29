use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MEMORY_CHAR_LIMIT: usize = 2_200;
pub const USER_CHAR_LIMIT: usize = 1_375;
const LEGACY_ENTRY_SEPARATOR: &str = "§";
const ENTRY_GAP: &str = "\n\n";
const STORAGE_FORMAT_MARKER: &str = "<!-- vela-memory-format: v2 -->";

#[derive(Debug, Clone)]
pub struct MemoryReport {
    pub memories_dir: PathBuf,
    pub memory_path: PathBuf,
    pub user_path: PathBuf,
    pub pending_dir: PathBuf,
    pub memory_exists_before: bool,
    pub user_exists_before: bool,
    pub memory_char_count: usize,
    pub user_char_count: usize,
    pub memory_char_limit: usize,
    pub user_char_limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryTarget {
    Memory,
    User,
}

impl MemoryTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::User => "user",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "memory" => Ok(Self::Memory),
            "user" => Ok(Self::User),
            other => bail!("invalid memory target {other:?}; expected 'memory' or 'user'"),
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::Memory => "MEMORY.md",
            Self::User => "USER.md",
        }
    }

    fn limit(self) -> usize {
        match self {
            Self::Memory => MEMORY_CHAR_LIMIT,
            Self::User => USER_CHAR_LIMIT,
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Memory => "MEMORY (personal notes)",
            Self::User => "USER PROFILE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    pub target: MemoryTarget,
    pub content: String,
    pub char_count: usize,
    pub char_limit: usize,
}

#[derive(Debug, Clone)]
pub struct MemoryView {
    pub target: MemoryTarget,
    pub entries: Vec<String>,
    pub char_count: usize,
    pub char_limit: usize,
}

#[derive(Debug, Clone)]
pub struct MemoryMutationReport {
    pub target: MemoryTarget,
    pub action: &'static str,
    pub char_count: usize,
    pub char_limit: usize,
    pub entry_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingMemoryWrite {
    pub id: String,
    pub target: MemoryTarget,
    pub action: String,
    pub old_text: Option<String>,
    pub matched_entry: Option<String>,
    pub new_text: Option<String>,
    pub created_at: i64,
}

pub fn initialize_memory(vela_home: &Path) -> Result<MemoryReport> {
    let memories_dir = vela_home.join("memories");
    fs::create_dir_all(&memories_dir)
        .with_context(|| format!("failed to create {}", memories_dir.display()))?;
    let pending_dir = vela_home.join("pending").join("memory");
    fs::create_dir_all(&pending_dir)
        .with_context(|| format!("failed to create {}", pending_dir.display()))?;

    let memory_path = memories_dir.join(MemoryTarget::Memory.filename());
    let user_path = memories_dir.join(MemoryTarget::User.filename());

    let memory_exists_before = memory_path.is_file();
    let user_exists_before = user_path.is_file();

    ensure_file(&memory_path)?;
    ensure_file(&user_path)?;
    migrate_legacy_file(&memory_path)?;
    migrate_legacy_file(&user_path)?;

    let memory_text = render_entries(&load_entries(vela_home, MemoryTarget::Memory)?);
    let user_text = render_entries(&load_entries(vela_home, MemoryTarget::User)?);

    Ok(MemoryReport {
        memories_dir,
        memory_path,
        user_path,
        pending_dir,
        memory_exists_before,
        user_exists_before,
        memory_char_count: memory_text.chars().count(),
        user_char_count: user_text.chars().count(),
        memory_char_limit: MEMORY_CHAR_LIMIT,
        user_char_limit: USER_CHAR_LIMIT,
    })
}

pub fn load_snapshot(vela_home: &Path, target: MemoryTarget) -> Result<MemorySnapshot> {
    let entries = load_entries(vela_home, target)?;
    let content = render_entries(&entries);
    Ok(MemorySnapshot {
        target,
        char_count: content.chars().count(),
        char_limit: target.limit(),
        content,
    })
}

pub fn view_memory(vela_home: &Path, target: MemoryTarget) -> Result<MemoryView> {
    let entries = load_entries(vela_home, target)?;
    let rendered = render_entries(&entries);
    Ok(MemoryView {
        target,
        entries,
        char_count: rendered.chars().count(),
        char_limit: target.limit(),
    })
}

pub fn add_memory_entry(vela_home: &Path, target: MemoryTarget, content: &str) -> Result<MemoryMutationReport> {
    let mut entries = load_entries(vela_home, target)?;
    let new_entry = sanitize_entry(content)?;
    entries.push(new_entry);
    save_entries(vela_home, target, &entries)?;
    report(target, "add", &entries)
}

pub fn stage_add_memory_entry(vela_home: &Path, target: MemoryTarget, content: &str) -> Result<PendingMemoryWrite> {
    let new_entry = sanitize_entry(content)?;
    let entries = load_entries(vela_home, target)?;
    if entries.iter().any(|entry| entry == &new_entry) {
        bail!("memory entry already exists for target {}", target.label());
    }
    if list_pending(vela_home)?.iter().any(|pending| {
        pending.target == target
            && pending.action == "add"
            && pending.new_text.as_deref() == Some(new_entry.as_str())
    }) {
        bail!("matching memory add is already pending approval for target {}", target.label());
    }
    stage_write(
        vela_home,
        PendingMemoryWrite {
            id: new_pending_id("mem"),
            target,
            action: "add".to_string(),
            old_text: None,
            matched_entry: None,
            new_text: Some(new_entry),
            created_at: unix_timestamp(),
        },
    )
}

pub fn replace_memory_entry(
    vela_home: &Path,
    target: MemoryTarget,
    old_text: &str,
    content: &str,
) -> Result<MemoryMutationReport> {
    let mut entries = load_entries(vela_home, target)?;
    let idx = unique_match_index(&entries, old_text)?;
    entries[idx] = sanitize_entry(content)?;
    save_entries(vela_home, target, &entries)?;
    report(target, "replace", &entries)
}

pub fn stage_replace_memory_entry(
    vela_home: &Path,
    target: MemoryTarget,
    old_text: &str,
    content: &str,
) -> Result<PendingMemoryWrite> {
    if old_text.trim().is_empty() {
        bail!("match text cannot be empty");
    }
    sanitize_entry(content)?;
    let entries = load_entries(vela_home, target)?;
    let idx = unique_match_index(&entries, old_text)?;
    stage_write(
        vela_home,
        PendingMemoryWrite {
            id: new_pending_id("mem"),
            target,
            action: "replace".to_string(),
            old_text: Some(old_text.trim().to_string()),
            matched_entry: Some(entries[idx].clone()),
            new_text: Some(content.trim().to_string()),
            created_at: unix_timestamp(),
        },
    )
}

pub fn remove_memory_entry(vela_home: &Path, target: MemoryTarget, old_text: &str) -> Result<MemoryMutationReport> {
    let mut entries = load_entries(vela_home, target)?;
    let idx = unique_match_index(&entries, old_text)?;
    entries.remove(idx);
    save_entries(vela_home, target, &entries)?;
    report(target, "remove", &entries)
}

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

fn remove_exact_memory_entry(vela_home: &Path, target: MemoryTarget, matched_entry: &str) -> Result<MemoryMutationReport> {
    let mut entries = load_entries(vela_home, target)?;
    let idx = exact_match_index(&entries, matched_entry)?;
    entries.remove(idx);
    save_entries(vela_home, target, &entries)?;
    report(target, "remove", &entries)
}

pub fn stage_remove_memory_entry(vela_home: &Path, target: MemoryTarget, old_text: &str) -> Result<PendingMemoryWrite> {
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

pub fn get_pending(vela_home: &Path, id: &str) -> Result<PendingMemoryWrite> {
    let id = validate_pending_id(id)?;
    let path = pending_dir(vela_home).join(format!("{id}.json"));
    let text = fs::read_to_string(&path)
        .with_context(|| format!("pending memory write {:?} not found", id))?;
    Ok(serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?)
}

pub fn reject_pending(vela_home: &Path, id: &str) -> Result<()> {
    let id = validate_pending_id(id)?;
    let path = pending_dir(vela_home).join(format!("{id}.json"));
    fs::remove_file(&path).with_context(|| format!("pending memory write {:?} not found", id))?;
    Ok(())
}

pub fn approve_pending(vela_home: &Path, id: &str) -> Result<MemoryMutationReport> {
    let pending = get_pending(vela_home, id)?;
    let result = match pending.action.as_str() {
        "add" => add_memory_entry(
            vela_home,
            pending.target,
            pending.new_text.as_deref().ok_or_else(|| anyhow!("pending add missing new_text"))?,
        )?,
        "replace" => replace_exact_memory_entry(
            vela_home,
            pending.target,
            pending.matched_entry.as_deref().ok_or_else(|| anyhow!("pending replace missing matched_entry"))?,
            pending.new_text.as_deref().ok_or_else(|| anyhow!("pending replace missing new_text"))?,
        )
        .with_context(|| format!("pending replace {} became stale or conflicted", pending.id))?,
        "remove" => remove_exact_memory_entry(
            vela_home,
            pending.target,
            pending.matched_entry.as_deref().ok_or_else(|| anyhow!("pending remove missing matched_entry"))?,
        )
        .with_context(|| format!("pending remove {} became stale or conflicted", pending.id))?,
        other => bail!("unknown pending memory action {other:?}"),
    };
    reject_pending(vela_home, id)?;
    Ok(result)
}

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

fn report(target: MemoryTarget, action: &'static str, entries: &[String]) -> Result<MemoryMutationReport> {
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
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
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
    fs::write(&path, render_storage(entries)).with_context(|| format!("failed to write {}", path.display()))?;
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
        format!("{STORAGE_FORMAT_MARKER}{ENTRY_GAP}{}", render_entries(entries))
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
        [] => Err(anyhow!("staged memory entry no longer exists exactly as reviewed")),
        _ => Err(anyhow!("staged memory entry became ambiguous; resolve manually")),
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
        _ => Err(anyhow!("memory match {needle:?} was ambiguous; use a more specific substring")),
    }
}

fn migrate_legacy_file(path: &Path) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders_round_trip_with_paragraph_entries() {
        let entries = vec![
            "First memory entry".to_string(),
            "Second memory entry\nwith another line".to_string(),
            "Third entry".to_string(),
        ];

        let rendered = render_entries(&entries);
        assert_eq!(rendered, "First memory entry\n\nSecond memory entry\nwith another line\n\nThird entry");
        assert_eq!(parse_entries(&rendered), entries);
        assert_eq!(parse_entries(&render_storage(&entries)), entries);
    }

    #[test]
    fn initialize_memory_migrates_legacy_separator_files() {
        let vela_home = std::env::temp_dir().join(format!("vela-memory-test-{}", unix_timestamp_nanos()));
        let memories_dir = vela_home.join("memories");
        fs::create_dir_all(&memories_dir).unwrap();
        fs::write(
            memories_dir.join("MEMORY.md"),
            "alpha§beta§gamma",
        )
        .unwrap();
        fs::write(memories_dir.join("USER.md"), "delta§epsilon").unwrap();

        let report = initialize_memory(&vela_home).unwrap();
        assert_eq!(report.memory_char_count, "alpha\n\nbeta\n\ngamma".chars().count());
        assert_eq!(report.user_char_count, "delta\n\nepsilon".chars().count());
        assert_eq!(
            fs::read_to_string(memories_dir.join("MEMORY.md")).unwrap(),
            "<!-- vela-memory-format: v2 -->\n\nalpha\n\nbeta\n\ngamma"
        );
        assert_eq!(
            fs::read_to_string(memories_dir.join("USER.md")).unwrap(),
            "<!-- vela-memory-format: v2 -->\n\ndelta\n\nepsilon"
        );

        fs::remove_dir_all(&vela_home).unwrap();
    }

    #[test]
    fn marker_prevents_false_legacy_migration_for_section_sign_entries() {
        let vela_home = std::env::temp_dir().join(format!("vela-memory-test-marker-{}", unix_timestamp_nanos()));
        let memories_dir = vela_home.join("memories");
        fs::create_dir_all(&memories_dir).unwrap();
        fs::write(
            memories_dir.join("MEMORY.md"),
            "<!-- vela-memory-format: v2 -->\n\nentry with § symbol",
        )
        .unwrap();
        fs::write(memories_dir.join("USER.md"), "<!-- vela-memory-format: v2 -->\n").unwrap();

        let view = view_memory(&vela_home, MemoryTarget::Memory).unwrap();
        assert_eq!(view.entries, vec!["entry with § symbol".to_string()]);
        assert_eq!(
            fs::read_to_string(memories_dir.join("MEMORY.md")).unwrap(),
            "<!-- vela-memory-format: v2 -->\n\nentry with § symbol"
        );

        fs::remove_dir_all(&vela_home).unwrap();
    }

    #[test]
    fn stage_add_rejects_duplicate_existing_or_pending_entries() {
        let vela_home = std::env::temp_dir().join(format!("vela-memory-test-dup-{}", unix_timestamp_nanos()));
        initialize_memory(&vela_home).unwrap();
        add_memory_entry(&vela_home, MemoryTarget::User, "remember this").unwrap();

        let err = stage_add_memory_entry(&vela_home, MemoryTarget::User, "remember this").unwrap_err();
        assert!(err.to_string().contains("already exists"));

        stage_add_memory_entry(&vela_home, MemoryTarget::User, "another memory").unwrap();
        let err = stage_add_memory_entry(&vela_home, MemoryTarget::User, "another memory").unwrap_err();
        assert!(err.to_string().contains("already pending approval"));

        fs::remove_dir_all(&vela_home).unwrap();
    }

    #[test]
    fn approve_pending_reports_stale_replace_clearly() {
        let vela_home = std::env::temp_dir().join(format!("vela-memory-test-stale-{}", unix_timestamp_nanos()));
        initialize_memory(&vela_home).unwrap();
        add_memory_entry(&vela_home, MemoryTarget::Memory, "old value").unwrap();
        let pending = stage_replace_memory_entry(&vela_home, MemoryTarget::Memory, "old", "new value").unwrap();
        replace_memory_entry(&vela_home, MemoryTarget::Memory, "old", "someone else changed it").unwrap();

        let err = approve_pending(&vela_home, &pending.id).unwrap_err();
        assert!(err.to_string().contains("became stale or conflicted"));

        fs::remove_dir_all(&vela_home).unwrap();
    }

    #[test]
    fn approve_pending_reports_stale_remove_clearly() {
        let vela_home = std::env::temp_dir().join(format!("vela-memory-test-stale-remove-{}", unix_timestamp_nanos()));
        initialize_memory(&vela_home).unwrap();
        add_memory_entry(&vela_home, MemoryTarget::Memory, "old value").unwrap();
        let pending = stage_remove_memory_entry(&vela_home, MemoryTarget::Memory, "old").unwrap();
        replace_memory_entry(&vela_home, MemoryTarget::Memory, "old", "someone else changed it").unwrap();

        let err = approve_pending(&vela_home, &pending.id).unwrap_err();
        assert!(err.to_string().contains("became stale or conflicted"));

        fs::remove_dir_all(&vela_home).unwrap();
    }
}
