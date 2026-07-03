use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod surface;

#[cfg(test)]
mod tests;

pub use surface::*;

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
    let lines: Vec<&str> = trimmed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
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
    let body = format!(
        "## When to use\n\n{}\n\n## Steps\n\n{}\n",
        title,
        step_lines.join("\n")
    );
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
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
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
