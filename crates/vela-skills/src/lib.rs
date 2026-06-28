use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SkillMutationReport {
    pub action: &'static str,
    pub name: String,
    pub skill_md_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SkillsReport {
    pub skills_dir: PathBuf,
    pub pending_dir: PathBuf,
    pub skills_dir_existed_before: bool,
    pub skill_count: usize,
}

#[derive(Debug, Clone)]
pub struct SkillSummary {
    pub name: String,
    pub skill_md_path: PathBuf,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SkillView {
    pub name: String,
    pub skill_md_path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSkillWrite {
    pub id: String,
    pub action: String,
    pub name: String,
    pub description: Option<String>,
    pub body: Option<String>,
    pub created_at: i64,
}

pub fn initialize_skills(vela_home: &Path) -> Result<SkillsReport> {
    let skills_dir = vela_home.join("skills");
    let existed_before = skills_dir.is_dir();
    fs::create_dir_all(&skills_dir)
        .with_context(|| format!("failed to create {}", skills_dir.display()))?;
    let pending_dir = vela_home.join("pending").join("skills");
    fs::create_dir_all(&pending_dir)
        .with_context(|| format!("failed to create {}", pending_dir.display()))?;
    let skills = list_skills(vela_home)?;
    Ok(SkillsReport {
        skills_dir,
        pending_dir,
        skills_dir_existed_before: existed_before,
        skill_count: skills.len(),
    })
}

pub fn list_skills(vela_home: &Path) -> Result<Vec<SkillSummary>> {
    let skills_dir = vela_home.join("skills");
    fs::create_dir_all(&skills_dir)
        .with_context(|| format!("failed to create {}", skills_dir.display()))?;

    let mut skills = Vec::new();
    for entry in fs::read_dir(&skills_dir)
        .with_context(|| format!("failed to read {}", skills_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let skill_md_path = path.join("SKILL.md");
        if !skill_md_path.is_file() {
            continue;
        }
        let description = extract_description(&skill_md_path)?;
        skills.push(SkillSummary {
            name,
            skill_md_path,
            description,
        });
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

pub fn view_skill(vela_home: &Path, name: &str) -> Result<SkillView> {
    let normalized = normalize_skill_name(name)?;
    let skill_md_path = skill_md_path(vela_home, &normalized);
    if !skill_md_path.is_file() {
        bail!("skill {:?} not found", normalized);
    }
    let content = fs::read_to_string(&skill_md_path)
        .with_context(|| format!("failed to read {}", skill_md_path.display()))?;
    Ok(SkillView {
        name: normalized,
        skill_md_path,
        content,
    })
}

pub fn create_skill(
    vela_home: &Path,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<SkillMutationReport> {
    let normalized = normalize_skill_name(name)?;
    let skill_dir = vela_home.join("skills").join(&normalized);
    let skill_md_path = skill_dir.join("SKILL.md");
    if skill_md_path.exists() {
        bail!("skill {:?} already exists", normalized);
    }
    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("failed to create {}", skill_dir.display()))?;
    fs::write(&skill_md_path, render_skill(&normalized, description, body))
        .with_context(|| format!("failed to write {}", skill_md_path.display()))?;
    Ok(SkillMutationReport {
        action: "create",
        name: normalized,
        skill_md_path,
    })
}

pub fn stage_create_skill(
    vela_home: &Path,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<PendingSkillWrite> {
    let normalized = normalize_skill_name(name)?;
    stage_write(
        vela_home,
        PendingSkillWrite {
            id: new_pending_id("skill"),
            action: "create".to_string(),
            name: normalized,
            description: description.map(|s| s.to_string()),
            body: body.map(|s| s.to_string()),
            created_at: unix_timestamp(),
        },
    )
}

pub fn write_skill(
    vela_home: &Path,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<SkillMutationReport> {
    let normalized = normalize_skill_name(name)?;
    let skill_md_path = skill_md_path(vela_home, &normalized);
    if !skill_md_path.is_file() {
        bail!("skill {:?} not found", normalized);
    }
    fs::write(&skill_md_path, render_skill(&normalized, description, body))
        .with_context(|| format!("failed to write {}", skill_md_path.display()))?;
    Ok(SkillMutationReport {
        action: "write",
        name: normalized,
        skill_md_path,
    })
}

pub fn stage_write_skill(
    vela_home: &Path,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
) -> Result<PendingSkillWrite> {
    let normalized = normalize_skill_name(name)?;
    stage_write(
        vela_home,
        PendingSkillWrite {
            id: new_pending_id("skill"),
            action: "write".to_string(),
            name: normalized,
            description: description.map(|s| s.to_string()),
            body: body.map(|s| s.to_string()),
            created_at: unix_timestamp(),
        },
    )
}

pub fn delete_skill(vela_home: &Path, name: &str) -> Result<SkillMutationReport> {
    let normalized = normalize_skill_name(name)?;
    let skill_dir = vela_home.join("skills").join(&normalized);
    let skill_md_path = skill_dir.join("SKILL.md");
    if !skill_md_path.is_file() {
        bail!("skill {:?} not found", normalized);
    }
    fs::remove_dir_all(&skill_dir)
        .with_context(|| format!("failed to delete {}", skill_dir.display()))?;
    Ok(SkillMutationReport {
        action: "delete",
        name: normalized,
        skill_md_path,
    })
}

pub fn stage_delete_skill(vela_home: &Path, name: &str) -> Result<PendingSkillWrite> {
    let normalized = normalize_skill_name(name)?;
    stage_write(
        vela_home,
        PendingSkillWrite {
            id: new_pending_id("skill"),
            action: "delete".to_string(),
            name: normalized,
            description: None,
            body: None,
            created_at: unix_timestamp(),
        },
    )
}

pub fn list_pending(vela_home: &Path) -> Result<Vec<PendingSkillWrite>> {
    let dir = pending_dir(vela_home);
    fs::create_dir_all(&dir)?;
    let mut items = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let pending: PendingSkillWrite = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        items.push(pending);
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(items)
}

pub fn get_pending(vela_home: &Path, id: &str) -> Result<PendingSkillWrite> {
    let path = pending_dir(vela_home).join(format!("{id}.json"));
    let text = fs::read_to_string(&path)
        .with_context(|| format!("pending skill write {:?} not found", id))?;
    Ok(serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?)
}

pub fn reject_pending(vela_home: &Path, id: &str) -> Result<()> {
    let path = pending_dir(vela_home).join(format!("{id}.json"));
    fs::remove_file(&path).with_context(|| format!("pending skill write {:?} not found", id))?;
    Ok(())
}

pub fn approve_pending(vela_home: &Path, id: &str) -> Result<SkillMutationReport> {
    let pending = get_pending(vela_home, id)?;
    let result = match pending.action.as_str() {
        "create" => create_skill(
            vela_home,
            &pending.name,
            pending.description.as_deref(),
            pending.body.as_deref(),
        )?,
        "write" => write_skill(
            vela_home,
            &pending.name,
            pending.description.as_deref(),
            pending.body.as_deref(),
        )?,
        "delete" => delete_skill(vela_home, &pending.name)?,
        other => bail!("unknown pending skill action {other:?}"),
    };
    reject_pending(vela_home, id)?;
    Ok(result)
}

fn pending_dir(vela_home: &Path) -> PathBuf {
    vela_home.join("pending").join("skills")
}

fn stage_write(vela_home: &Path, pending: PendingSkillWrite) -> Result<PendingSkillWrite> {
    let dir = pending_dir(vela_home);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", pending.id));
    fs::write(&path, serde_json::to_string_pretty(&pending)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(pending)
}

fn skill_md_path(vela_home: &Path, name: &str) -> PathBuf {
    vela_home.join("skills").join(name).join("SKILL.md")
}

fn normalize_skill_name(name: &str) -> Result<String> {
    let normalized = name.trim();
    if normalized.is_empty() {
        bail!("skill name cannot be empty");
    }
    if normalized.contains('/') || normalized.contains('\\') {
        bail!("skill name cannot contain path separators");
    }
    Ok(normalized.to_string())
}

fn render_skill(name: &str, description: Option<&str>, body: Option<&str>) -> String {
    let description = description
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Procedural memory scaffold.");
    let body = body
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("## When to use\n\nDescribe when this skill should be used.\n\n## Steps\n\nDescribe the procedure here.\n");
    format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n")
}

fn extract_description(path: &Path) -> Result<Option<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "---" {
            break;
        }
    }
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("description:") {
            let desc = value.trim();
            if !desc.is_empty() {
                return Ok(Some(desc.to_string()));
            }
        }
    }
    Ok(None)
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
