use super::*;

#[derive(Debug, Clone)]
/// Represents `MemoryReport` data exposed by this crate.
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
/// Enumerates supported `MemoryTarget` variants.
pub enum MemoryTarget {
    Memory,
    User,
}

impl MemoryTarget {
    /// Returns the stable string label used for persistence and display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::User => "user",
        }
    }

    /// Parses a persisted or user-provided value into the corresponding enum.
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "memory" => Ok(Self::Memory),
            "user" => Ok(Self::User),
            other => bail!("invalid memory target {other:?}; expected 'memory' or 'user'"),
        }
    }

    pub(crate) fn filename(self) -> &'static str {
        match self {
            Self::Memory => "MEMORY.md",
            Self::User => "USER.md",
        }
    }

    pub(crate) fn limit(self) -> usize {
        match self {
            Self::Memory => MEMORY_CHAR_LIMIT,
            Self::User => USER_CHAR_LIMIT,
        }
    }

    pub(crate) fn heading(self) -> &'static str {
        match self {
            Self::Memory => "MEMORY (personal notes)",
            Self::User => "USER PROFILE",
        }
    }
}

#[derive(Debug, Clone)]
/// Represents `MemorySnapshot` data exposed by this crate.
pub struct MemorySnapshot {
    pub target: MemoryTarget,
    pub content: String,
    pub char_count: usize,
    pub char_limit: usize,
}

#[derive(Debug, Clone)]
/// Represents `MemoryView` data exposed by this crate.
pub struct MemoryView {
    pub target: MemoryTarget,
    pub entries: Vec<String>,
    pub char_count: usize,
    pub char_limit: usize,
}

#[derive(Debug, Clone)]
/// Represents `MemoryMutationReport` data exposed by this crate.
pub struct MemoryMutationReport {
    pub target: MemoryTarget,
    pub action: &'static str,
    pub char_count: usize,
    pub char_limit: usize,
    pub entry_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents `PendingMemoryWrite` data exposed by this crate.
pub struct PendingMemoryWrite {
    pub id: String,
    pub target: MemoryTarget,
    pub action: String,
    pub old_text: Option<String>,
    pub matched_entry: Option<String>,
    pub new_text: Option<String>,
    pub created_at: i64,
}

/// Initializes memory state for this subsystem.
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

/// Loads snapshot from durable storage.
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

/// Returns a view of memory content.
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

/// Adds memory entry to durable storage.
pub fn add_memory_entry(
    vela_home: &Path,
    target: MemoryTarget,
    content: &str,
) -> Result<MemoryMutationReport> {
    let mut entries = load_entries(vela_home, target)?;
    let new_entry = sanitize_entry(content)?;
    entries.push(new_entry);
    save_entries(vela_home, target, &entries)?;
    report(target, "add", &entries)
}

/// Stages add memory entry for explicit approval.
pub fn stage_add_memory_entry(
    vela_home: &Path,
    target: MemoryTarget,
    content: &str,
) -> Result<PendingMemoryWrite> {
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
        bail!(
            "matching memory add is already pending approval for target {}",
            target.label()
        );
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

/// Replaces memory entry in durable storage.
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

/// Stages replace memory entry for explicit approval.
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

/// Removes memory entry from durable storage.
pub fn remove_memory_entry(
    vela_home: &Path,
    target: MemoryTarget,
    old_text: &str,
) -> Result<MemoryMutationReport> {
    let mut entries = load_entries(vela_home, target)?;
    let idx = unique_match_index(&entries, old_text)?;
    entries.remove(idx);
    save_entries(vela_home, target, &entries)?;
    report(target, "remove", &entries)
}
