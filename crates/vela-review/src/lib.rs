use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ReviewReport {
    pub reviews_dir: PathBuf,
    pub candidates_dir: PathBuf,
    pub reviews_dir_existed_before: bool,
    pub candidate_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ReviewKind {
    Memory,
    Skill,
}

impl ReviewKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewCandidate {
    pub id: String,
    pub kind: ReviewKind,
    pub source: String,
    pub reason: String,
    pub created_at: i64,
    #[serde(alias = "session_id")]
    pub origin_session_id: Option<String>,
    pub origin_session_title: Option<String>,
    pub memory: Option<MemoryCandidate>,
    pub skill: Option<SkillCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub action: String,
    pub target: vela_memory::MemoryTarget,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCandidate {
    pub action: String,
    pub name: String,
    pub description: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PromotionReport {
    pub candidate_id: String,
    pub kind: ReviewKind,
    pub pending_id: String,
}

#[derive(Debug, Clone)]
pub struct SuggestionMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SuggestionEvent {
    pub event_type: String,
    pub payload_json: String,
}

#[derive(Debug, Clone)]
pub struct SuggestionInput {
    pub session_id: String,
    pub session_title: String,
    pub messages: Vec<SuggestionMessage>,
    pub events: Vec<SuggestionEvent>,
}

#[derive(Debug, Clone)]
pub struct SuggestionReport {
    pub session_id: String,
    pub session_title: String,
    pub candidate_ids: Vec<String>,
    pub skipped: usize,
}

#[derive(Debug, Clone)]
pub struct SignalSpec {
    pub event_type: String,
    pub payload_json: String,
}

#[derive(Debug, Clone)]
pub struct SignalReport {
    pub session_id: String,
    pub session_title: String,
    pub signals: Vec<SignalSpec>,
    pub skipped: usize,
}

pub fn initialize_reviews(vela_home: &Path) -> Result<ReviewReport> {
    let reviews_dir = vela_home.join("reviews");
    let existed_before = reviews_dir.is_dir();
    let candidates_dir = reviews_dir.join("candidates");
    fs::create_dir_all(&candidates_dir)
        .with_context(|| format!("failed to create {}", candidates_dir.display()))?;
    let candidate_count = list_candidates(vela_home)?.len();
    Ok(ReviewReport {
        reviews_dir,
        candidates_dir,
        reviews_dir_existed_before: existed_before,
        candidate_count,
    })
}

pub fn stage_memory_candidate(
    vela_home: &Path,
    target: vela_memory::MemoryTarget,
    action: &str,
    old_text: Option<&str>,
    new_text: Option<&str>,
    reason: &str,
    source: Option<&str>,
    origin_session_id: Option<&str>,
    origin_session_title: Option<&str>,
) -> Result<ReviewCandidate> {
    let action = normalize_action(action, &["add", "replace", "remove"], "memory")?;
    if action == "replace" && old_text.unwrap_or_default().trim().is_empty() {
        bail!("memory review replace requires match text");
    }
    if (action == "add" || action == "replace") && new_text.unwrap_or_default().trim().is_empty() {
        bail!("memory review {action} requires new text");
    }
    if action == "remove" && old_text.unwrap_or_default().trim().is_empty() {
        bail!("memory review remove requires match text");
    }
    let candidate = ReviewCandidate {
        id: new_candidate_id(),
        kind: ReviewKind::Memory,
        source: normalize_source(source),
        reason: normalize_reason(reason),
        created_at: unix_timestamp(),
        origin_session_id: origin_session_id.map(|s| s.to_string()),
        origin_session_title: origin_session_title.map(|s| s.to_string()),
        memory: Some(MemoryCandidate {
            action,
            target,
            old_text: old_text.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            new_text: new_text.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        }),
        skill: None,
    };
    write_candidate(vela_home, &candidate)?;
    Ok(candidate)
}

pub fn stage_skill_candidate(
    vela_home: &Path,
    action: &str,
    name: &str,
    description: Option<&str>,
    body: Option<&str>,
    reason: &str,
    source: Option<&str>,
    origin_session_id: Option<&str>,
    origin_session_title: Option<&str>,
) -> Result<ReviewCandidate> {
    let normalized_name = vela_skills::normalize_skill_name(name)
        .context("skill review requires a valid promotable skill name")?;
    let action = normalize_action(action, &["create", "write", "delete"], "skill")?;
    let candidate = ReviewCandidate {
        id: new_candidate_id(),
        kind: ReviewKind::Skill,
        source: normalize_source(source),
        reason: normalize_reason(reason),
        created_at: unix_timestamp(),
        origin_session_id: origin_session_id.map(|s| s.to_string()),
        origin_session_title: origin_session_title.map(|s| s.to_string()),
        memory: None,
        skill: Some(SkillCandidate {
            action,
            name: normalized_name,
            description: description.map(|s| s.to_string()),
            body: body.map(|s| s.to_string()),
        }),
    };
    write_candidate(vela_home, &candidate)?;
    Ok(candidate)
}

pub fn list_candidates(vela_home: &Path) -> Result<Vec<ReviewCandidate>> {
    let dir = candidates_dir(vela_home);
    fs::create_dir_all(&dir)?;
    let mut items = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let candidate: ReviewCandidate = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        items.push(candidate);
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(items)
}

pub fn get_candidate(vela_home: &Path, id: &str) -> Result<ReviewCandidate> {
    let id = validate_candidate_id(id)?;
    let path = candidates_dir(vela_home).join(format!("{id}.json"));
    let text = fs::read_to_string(&path)
        .with_context(|| format!("review candidate {:?} not found", id))?;
    Ok(serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?)
}

pub fn reject_candidate(vela_home: &Path, id: &str) -> Result<ReviewCandidate> {
    let candidate = get_candidate(vela_home, id)?;
    let id = validate_candidate_id(id)?;
    let path = candidates_dir(vela_home).join(format!("{id}.json"));
    fs::remove_file(&path).with_context(|| format!("review candidate {:?} not found", id))?;
    Ok(candidate)
}

pub fn infer_signals(input: &SuggestionInput) -> Result<SignalReport> {
    let mut seen = HashSet::new();
    for event in &input.events {
        if (event.event_type == "memory_signal" || event.event_type == "skill_signal")
            && !event.payload_json.trim().is_empty()
        {
            seen.insert(format!("{}::{}", event.event_type, normalize_dedupe_text(&event.payload_json)));
        }
    }

    let mut signals = Vec::new();
    let mut skipped = 0usize;
    let user_messages: Vec<&SuggestionMessage> = input.messages.iter().filter(|m| m.role == "user").collect();

    for message in &user_messages {
        if let Some(candidate_text) = infer_memory_candidate_from_message(&message.content) {
            let payload = json!({
                "action": "add",
                "target": "user",
                "new_text": candidate_text,
                "reason": format!("Auto-emitted durable preference from session {}.", input.session_title),
                "source": "auto-transcript-preference"
            });
            let key = format!("memory_signal::{}", normalize_dedupe_text(&payload.to_string()));
            if seen.insert(key) {
                signals.push(SignalSpec { event_type: "memory_signal".to_string(), payload_json: payload.to_string() });
            } else {
                skipped += 1;
            }
        }
    }

    let correction_messages: Vec<&SuggestionMessage> = user_messages
        .iter()
        .copied()
        .filter(|m| looks_like_correction(&m.content))
        .collect();
    if correction_messages.len() >= 2 {
        if let Some(last) = correction_messages.last() {
            let payload = json!({
                "action": "add",
                "target": "user",
                "new_text": last.content.trim(),
                "reason": format!("Auto-emitted repeated correction from session {}.", input.session_title),
                "source": "auto-repeated-correction"
            });
            let key = format!("memory_signal::{}", normalize_dedupe_text(&payload.to_string()));
            if seen.insert(key) {
                signals.push(SignalSpec { event_type: "memory_signal".to_string(), payload_json: payload.to_string() });
            } else {
                skipped += 1;
            }
        }
    }

    for message in &user_messages {
        if let Some((name, description, body)) = infer_skill_from_message(&message.content) {
            let payload = json!({
                "action": "create",
                "name": name,
                "description": description,
                "body": body,
                "reason": format!("Auto-emitted procedural skill candidate from session {}.", input.session_title),
                "source": "auto-procedure-capture"
            });
            let key = format!("skill_signal::{}", normalize_dedupe_text(&payload.to_string()));
            if seen.insert(key) {
                signals.push(SignalSpec { event_type: "skill_signal".to_string(), payload_json: payload.to_string() });
            } else {
                skipped += 1;
            }
        }
    }

    Ok(SignalReport {
        session_id: input.session_id.clone(),
        session_title: input.session_title.clone(),
        signals,
        skipped,
    })
}

pub fn generate_candidates(vela_home: &Path, input: SuggestionInput) -> Result<SuggestionReport> {
    let existing_review_candidates = list_candidates(vela_home)?;
    let pending_memory = vela_memory::list_pending(vela_home)?;
    let pending_skills = vela_skills::list_pending(vela_home)?;
    let user_memory = vela_memory::view_memory(vela_home, vela_memory::MemoryTarget::User)?;
    let core_memory = vela_memory::view_memory(vela_home, vela_memory::MemoryTarget::Memory)?;
    let existing_skills = vela_skills::list_skills(vela_home)?;

    let mut memory_seen: HashSet<String> = user_memory
        .entries
        .iter()
        .chain(core_memory.entries.iter())
        .map(|s| normalize_dedupe_text(s))
        .collect();
    for item in &pending_memory {
        if let Some(text) = item.new_text.as_deref() {
            memory_seen.insert(normalize_dedupe_text(text));
        }
    }
    for candidate in &existing_review_candidates {
        if let Some(memory) = candidate.memory.as_ref() {
            if let Some(text) = memory.new_text.as_deref() {
                memory_seen.insert(normalize_dedupe_text(text));
            }
        }
    }

    let mut skill_seen: HashSet<String> = existing_skills.into_iter().map(|s| s.name).collect();
    for item in &pending_skills {
        skill_seen.insert(item.name.clone());
    }
    for candidate in &existing_review_candidates {
        if let Some(skill) = candidate.skill.as_ref() {
            skill_seen.insert(skill.name.clone());
        }
    }

    let mut candidate_ids = Vec::new();
    let mut skipped = 0usize;

    for message in input.messages.iter().filter(|m| m.role == "user") {
        if let Some(candidate_text) = infer_memory_candidate_from_message(&message.content) {
            let key = normalize_dedupe_text(&candidate_text);
            if memory_seen.contains(&key) {
                skipped += 1;
            } else {
                let candidate = stage_memory_candidate(
                    vela_home,
                    vela_memory::MemoryTarget::User,
                    "add",
                    None,
                    Some(&candidate_text),
                    &format!("Transcript-derived preference from session {}.", input.session_title),
                    Some("session-transcript"),
                    Some(&input.session_id),
                    Some(&input.session_title),
                )?;
                memory_seen.insert(key);
                candidate_ids.push(candidate.id);
            }
        }
    }

    for event in &input.events {
        if event.event_type == "memory_signal" {
            if let Some(payload) = parse_event_payload(&event.payload_json) {
                let action = payload.get("action").and_then(Value::as_str).unwrap_or("add");
                let target = payload
                    .get("target")
                    .and_then(Value::as_str)
                    .map(vela_memory::MemoryTarget::parse)
                    .transpose()?
                    .unwrap_or(vela_memory::MemoryTarget::User);
                let old_text = payload.get("old_text").and_then(Value::as_str);
                let new_text = payload.get("new_text").and_then(Value::as_str);
                if let Some(text) = new_text {
                    let key = normalize_dedupe_text(text);
                    if action == "add" && memory_seen.contains(&key) {
                        skipped += 1;
                        continue;
                    }
                }
                let candidate = stage_memory_candidate(
                    vela_home,
                    target,
                    action,
                    old_text,
                    new_text,
                    payload
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("Structured memory signal from session event."),
                    payload.get("source").and_then(Value::as_str),
                    Some(&input.session_id),
                    Some(&input.session_title),
                )?;
                if let Some(text) = new_text {
                    memory_seen.insert(normalize_dedupe_text(text));
                }
                candidate_ids.push(candidate.id);
            }
        } else if event.event_type == "skill_signal" {
            if let Some(payload) = parse_event_payload(&event.payload_json) {
                let action = match normalize_action(
                    payload.get("action").and_then(Value::as_str).unwrap_or("create"),
                    &["create", "write", "delete"],
                    "skill",
                ) {
                    Ok(action) => action,
                    Err(_) => {
                        skipped += 1;
                        continue;
                    }
                };
                let name = payload.get("name").and_then(Value::as_str).unwrap_or("");
                let normalized_name = match vela_skills::normalize_skill_name(name) {
                    Ok(name) => name,
                    Err(_) => {
                        skipped += 1;
                        continue;
                    }
                };
                if action == "create" && skill_seen.contains(&normalized_name) {
                    skipped += 1;
                    continue;
                }
                let candidate = stage_skill_candidate(
                    vela_home,
                    &action,
                    name,
                    payload.get("description").and_then(Value::as_str),
                    payload.get("body").and_then(Value::as_str),
                    payload
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("Structured skill signal from session event."),
                    payload.get("source").and_then(Value::as_str),
                    Some(&input.session_id),
                    Some(&input.session_title),
                )?;
                skill_seen.insert(candidate.skill.as_ref().map(|s| s.name.clone()).unwrap_or_default());
                candidate_ids.push(candidate.id);
            }
        }
    }

    Ok(SuggestionReport {
        session_id: input.session_id,
        session_title: input.session_title,
        candidate_ids,
        skipped,
    })
}

pub fn promote_candidate(vela_home: &Path, id: &str) -> Result<PromotionReport> {
    let candidate = get_candidate(vela_home, id)?;
    let report = match candidate.kind {
        ReviewKind::Memory => {
            let memory = candidate.memory.as_ref().context("memory candidate payload missing")?;
            let pending = match memory.action.as_str() {
                "add" => vela_memory::stage_add_memory_entry(
                    vela_home,
                    memory.target,
                    memory.new_text.as_deref().context("memory add candidate missing new_text")?,
                )
                .with_context(|| format!("memory review candidate {} conflicted with existing or pending memory state", candidate.id))?,
                "replace" => vela_memory::stage_replace_memory_entry(
                    vela_home,
                    memory.target,
                    memory.old_text.as_deref().context("memory replace candidate missing old_text")?,
                    memory.new_text.as_deref().context("memory replace candidate missing new_text")?,
                )
                .with_context(|| format!("memory review candidate {} became stale or conflicted before promotion", candidate.id))?,
                "remove" => vela_memory::stage_remove_memory_entry(
                    vela_home,
                    memory.target,
                    memory.old_text.as_deref().context("memory remove candidate missing old_text")?,
                )
                .with_context(|| format!("memory review candidate {} became stale or conflicted before promotion", candidate.id))?,
                other => bail!("unknown memory review action {other:?}"),
            };
            PromotionReport {
                candidate_id: candidate.id.clone(),
                kind: ReviewKind::Memory,
                pending_id: pending.id,
            }
        }
        ReviewKind::Skill => {
            let skill = candidate.skill.as_ref().context("skill candidate payload missing")?;
            let pending = match skill.action.as_str() {
                "create" => vela_skills::stage_create_skill(
                    vela_home,
                    &skill.name,
                    skill.description.as_deref(),
                    skill.body.as_deref(),
                )
                .with_context(|| format!("skill review candidate {} conflicted with existing or pending skill state", candidate.id))?,
                "write" => vela_skills::stage_write_skill(
                    vela_home,
                    &skill.name,
                    skill.description.as_deref(),
                    skill.body.as_deref(),
                )
                .with_context(|| format!("skill review candidate {} conflicted with existing or pending skill state", candidate.id))?,
                "delete" => vela_skills::stage_delete_skill(vela_home, &skill.name)
                    .with_context(|| format!("skill review candidate {} conflicted with existing or pending skill state", candidate.id))?,
                other => bail!("unknown skill review action {other:?}"),
            };
            PromotionReport {
                candidate_id: candidate.id.clone(),
                kind: ReviewKind::Skill,
                pending_id: pending.id,
            }
        }
    };
    reject_candidate(vela_home, id)?;
    Ok(report)
}

fn infer_memory_candidate_from_message(content: &str) -> Option<String> {
    let first_line = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    if first_line.chars().count() > 160 {
        return None;
    }
    let lower = first_line.to_ascii_lowercase();
    let looks_like_preference = lower.starts_with("please ")
        || lower.starts_with("remember ")
        || lower.contains("prefer ")
        || lower.contains("always ")
        || lower.contains("never ");
    if !looks_like_preference {
        return None;
    }
    Some(first_line.to_string())
}

fn looks_like_correction(content: &str) -> bool {
    let lower = content.trim().to_ascii_lowercase();
    lower.starts_with("actually ")
        || lower.contains(" instead ")
        || lower.contains(" rather than ")
        || lower.contains(" not ") && lower.contains(" use ")
}

fn infer_skill_from_message(content: &str) -> Option<(String, String, String)> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lines: Vec<&str> = trimmed.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let step_indexes: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            let bytes = line.as_bytes();
            (!bytes.is_empty() && bytes[0].is_ascii_digit() && line.contains('.')).then_some(idx)
        })
        .collect();
    if step_indexes.len() < 3 {
        return None;
    }
    let first_step = *step_indexes.first()?;
    let step_lines: Vec<&str> = step_indexes.iter().map(|idx| lines[*idx]).collect();
    let title_line = if first_step > 0 {
        lines[first_step - 1]
    } else {
        "captured procedure"
    };
    let title = title_line.trim_end_matches(':').trim();
    let name = slugify(title.trim_end_matches(" procedure"));
    if name.is_empty() {
        return None;
    }
    let description = format!("Captured procedure from session text: {}", title);
    let body = format!("## When to use\n\n{}\n\n## Steps\n\n{}\n", title, step_lines.join("\n"));
    Some((name, description, body))
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn parse_event_payload(payload_json: &str) -> Option<Value> {
    serde_json::from_str(payload_json).ok()
}

fn normalize_dedupe_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase()
}

fn validate_candidate_id(id: &str) -> Result<&str> {
    let id = id.trim();
    if id.is_empty() || id == "." || id == ".." || id.contains('/') || id.contains('\\') {
        bail!("invalid review candidate id");
    }
    Ok(id)
}

fn write_candidate(vela_home: &Path, candidate: &ReviewCandidate) -> Result<()> {
    let dir = candidates_dir(vela_home);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", candidate.id));
    fs::write(&path, serde_json::to_string_pretty(candidate)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn candidates_dir(vela_home: &Path) -> PathBuf {
    vela_home.join("reviews").join("candidates")
}

fn normalize_action(action: &str, allowed: &[&str], subject: &str) -> Result<String> {
    let action = action.trim().to_ascii_lowercase();
    if allowed.contains(&action.as_str()) {
        Ok(action)
    } else {
        bail!("invalid {subject} review action {action:?}")
    }
}

fn normalize_reason(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        "Background review suggestion.".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_source(source: Option<&str>) -> String {
    source
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("background-review")
        .to_string()
}

fn new_candidate_id() -> String {
    format!("review-{}", unix_timestamp_nanos())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_candidates_skips_duplicate_memory_work_on_repeat_passes() {
        let vela_home = std::env::temp_dir().join(format!("vela-review-test-{}", unix_timestamp_nanos()));
        vela_memory::initialize_memory(&vela_home).unwrap();
        vela_skills::initialize_skills(&vela_home).unwrap();
        initialize_reviews(&vela_home).unwrap();

        let input = SuggestionInput {
            session_id: "session-1".to_string(),
            session_title: "Session 1".to_string(),
            messages: vec![SuggestionMessage {
                role: "user".to_string(),
                content: "Please remember that I prefer concise responses.".to_string(),
            }],
            events: vec![],
        };

        let first = generate_candidates(&vela_home, input.clone()).unwrap();
        assert_eq!(first.candidate_ids.len(), 1);

        let second = generate_candidates(&vela_home, input).unwrap();
        assert!(second.candidate_ids.is_empty());
        assert!(second.skipped >= 1);

        fs::remove_dir_all(&vela_home).unwrap();
    }
}
