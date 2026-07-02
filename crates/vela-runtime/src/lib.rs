use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::thread::sleep;
use std::time::Duration;
use vela_config::{BootstrapConfig, ConfigSource, ResolvedConfig};
use vela_extensions::ExtensionsReport;
use vela_memory::MemoryReport;
use vela_review::ReviewReport;
use vela_skills::SkillsReport;
use vela_state::{PersistenceReport, SessionRuntimeReport};

pub use vela_config::preparse_profile_override;
pub use vela_extensions::{
    ExtensionActivation, ExtensionKind, ExtensionLifecycle, ExtensionRecord,
};
pub use vela_state::{
    InteractionMode, RuntimeTurnLifecycleRecord, SessionAction, SessionBranchRecord,
    SessionCompressionRecord, SessionEventRecord, SessionInspection, SessionMessageRecord,
    SessionSearchHit, SessionSummary,
};

mod surface;

#[cfg(test)]
mod tests;

pub use surface::*;

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
    append_review_event(
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
    )?;
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
    append_review_event(
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
    )?;
    Ok(candidate)
}

/// Promotes a review candidate into the appropriate pending approval queue.
pub fn promote_review_candidate(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<vela_review::PromotionReport> {
    let candidate = vela_review::get_candidate(&bootstrap.vela_home, id)?;
    let report = vela_review::promote_candidate(&bootstrap.vela_home, id)?;
    append_review_event(
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
    )?;
    Ok(report)
}

/// Rejects a queued review candidate.
pub fn reject_review_candidate(bootstrap: &BootstrapReport, id: &str) -> Result<()> {
    let candidate = vela_review::reject_candidate(&bootstrap.vela_home, id)?;
    append_review_event(
        bootstrap,
        candidate.origin_session_id.as_deref(),
        "review_candidate_rejected",
        json!({
            "candidate_id": id,
            "origin_session_id": candidate.origin_session_id.clone(),
            "origin_session_title": candidate.origin_session_title.clone(),
        })
        .to_string(),
    )?;
    Ok(())
}

/// Infers review signals from the latest session and appends them as events.
pub fn emit_review_signals_from_latest_session(
    bootstrap: &BootstrapReport,
    limit: usize,
) -> Result<Option<vela_review::SignalReport>> {
    let Some(session) =
        vela_state::inspect_latest_session(&bootstrap.persistence.state_db_path, limit)?
    else {
        return Ok(None);
    };
    let input = vela_review::SuggestionInput {
        session_id: session.session_id.clone(),
        session_title: session.title.clone(),
        messages: session
            .messages
            .iter()
            .map(|m| vela_review::SuggestionMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect(),
        events: session
            .events
            .iter()
            .map(|e| vela_review::SuggestionEvent {
                event_type: e.event_type.clone(),
                payload_json: e.payload_json.clone(),
            })
            .collect(),
    };
    let report = vela_review::infer_signals(&input)?;
    for signal in &report.signals {
        let logged = vela_state::append_event_to_session(
            &bootstrap.persistence.state_db_path,
            &report.session_id,
            &signal.event_type,
            signal.payload_json.clone(),
        )?;
        if !logged {
            tracing::warn!("failed to append review signal to inspected session");
        }
    }
    let logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &report.session_id,
        "review_signals_emitted",
        json!({
            "session_id": report.session_id,
            "signal_count": report.signals.len(),
            "skipped": report.skipped,
        })
        .to_string(),
    )?;
    if !logged {
        tracing::warn!("failed to append review_signals_emitted event to inspected session");
    }
    Ok(Some(report))
}

struct RenderedChatResponse {
    content: Option<String>,
    source: &'static str,
    provider: Option<String>,
    model: Option<String>,
}

trait RuntimeProviderBackend {
    fn label(&self) -> &str;
    fn model(&self) -> Option<&str>;
    fn validate(&self) -> Result<()>;
    fn generate(&self, prompt: &str, images: Option<Vec<String>>) -> Result<String>;
    fn direct_response_source(&self) -> &'static str;
    fn tool_loop_response_source(&self) -> &'static str;
}

#[derive(Debug, Clone)]
struct OllamaRuntimeProvider {
    label: String,
    model: Option<String>,
    base_url: String,
}

impl RuntimeProviderBackend for OllamaRuntimeProvider {
    fn label(&self) -> &str {
        &self.label
    }

    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn validate(&self) -> Result<()> {
        validate_ollama_base_url(&self.base_url)
    }

    fn generate(&self, prompt: &str, images: Option<Vec<String>>) -> Result<String> {
        let model = self.model.as_deref().context(
            "runtime provider 'ollama' requires a model (for example a Gemma family model)",
        )?;
        call_ollama_generate(&self.base_url, model, prompt, images)
    }

    fn direct_response_source(&self) -> &'static str {
        "runtime-ollama"
    }

    fn tool_loop_response_source(&self) -> &'static str {
        "runtime-ollama-tool-loop"
    }
}

#[derive(Debug, Clone, Copy)]
enum RuntimeToolName {
    MemorySnapshot,
    ListSkills,
    ViewMemory,
    SearchSessionHistory,
    ViewSkill,
}

impl RuntimeToolName {
    fn as_str(self) -> &'static str {
        match self {
            Self::MemorySnapshot => "memory_snapshot",
            Self::ListSkills => "list_skills",
            Self::ViewMemory => "view_memory",
            Self::SearchSessionHistory => "search_session_history",
            Self::ViewSkill => "view_skill",
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeToolInvocation {
    name: RuntimeToolName,
    target: Option<vela_memory::MemoryTarget>,
    query: Option<String>,
    skill_name: Option<String>,
    limit: Option<usize>,
}

impl RuntimeToolInvocation {
    fn display_name(&self) -> &'static str {
        self.name.as_str()
    }

    fn request_text(&self) -> String {
        match self.name {
            RuntimeToolName::MemorySnapshot | RuntimeToolName::ListSkills => {
                self.display_name().to_string()
            }
            RuntimeToolName::ViewMemory => format!(
                "{}:{}",
                self.display_name(),
                self.target
                    .unwrap_or(vela_memory::MemoryTarget::Memory)
                    .label()
            ),
            RuntimeToolName::SearchSessionHistory => format!(
                "{}:{}",
                self.display_name(),
                self.query.as_deref().unwrap_or_default()
            ),
            RuntimeToolName::ViewSkill => format!(
                "{}:{}",
                self.display_name(),
                self.skill_name.as_deref().unwrap_or_default()
            ),
        }
    }

    fn metadata_json(&self) -> serde_json::Value {
        json!({
            "tool": self.display_name(),
            "target": self.target.map(|target| target.label().to_string()),
            "query": self.query,
            "skill_name": self.skill_name,
            "limit": self.limit,
        })
    }
}

const MAX_RUNTIME_TOOL_STEPS: usize = 3;
const MAX_RUNTIME_REFLECTION_ATTEMPTS: usize = 2;

struct RuntimeExecutionConfig {
    provider: Option<Box<dyn RuntimeProviderBackend>>,
    provider_label: Option<String>,
    model: Option<String>,
}

struct RuntimeTurnRecorder {
    turn_id: String,
    next_sequence: u64,
    final_phase: Option<String>,
}

impl RuntimeTurnRecorder {
    fn new() -> Self {
        Self {
            turn_id: format!("turn-{}", unix_timestamp_nanos()),
            next_sequence: 0,
            final_phase: None,
        }
    }

    fn record_phase(
        &mut self,
        bootstrap: &BootstrapReport,
        session_id: &str,
        phase: &str,
        step: Option<usize>,
        detail: serde_json::Value,
    ) -> Result<()> {
        self.next_sequence += 1;
        let payload = json!({
            "turn_id": self.turn_id,
            "sequence": self.next_sequence,
            "phase": phase,
            "step": step,
            "detail": detail,
        })
        .to_string();
        let logged = vela_state::append_event_to_session(
            &bootstrap.persistence.state_db_path,
            session_id,
            "runtime_turn_phase",
            payload,
        )?;
        if !logged {
            bail!(
                "failed to persist runtime turn lifecycle phase {:?} for session {:?}",
                phase,
                session_id
            );
        }
        self.final_phase = Some(phase.to_string());
        Ok(())
    }

    fn phase_count(&self) -> usize {
        self.next_sequence as usize
    }

    fn final_phase(&self) -> &str {
        self.final_phase.as_deref().unwrap_or("unknown")
    }
}

#[derive(Debug, Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeToolRequest {
    tool: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone)]
enum ProviderContinuation {
    FinalAnswer,
    ToolRequest(RuntimeToolInvocation),
    InvalidToolRequest,
    EmptyResponse,
}

enum ReflectionOutcome {
    RetryPrompt(String),
    Fallback(RenderedChatResponse),
}

fn render_chat_response(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    request: &SessionRequest,
    provider_override: Option<&str>,
    model_override: Option<&str>,
    lifecycle: &mut RuntimeTurnRecorder,
) -> Result<RenderedChatResponse> {
    let execution = resolve_runtime_execution(
        &bootstrap.resolved_config,
        provider_override,
        model_override,
    )?;

    let memory = vela_memory::render_prompt_snapshot(&bootstrap.vela_home)?;
    let skills = vela_skills::list_skills(&bootstrap.vela_home)?;
    let reviews = vela_review::list_candidates(&bootstrap.vela_home)?;
    let compression_summary = vela_state::latest_compression_summary(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
    )?;
    let compression_block = compression_summary
        .as_deref()
        .map(|summary| format!("\n\nCompressed continuity summary:\n{}", summary))
        .unwrap_or_default();
    let memory_lines = memory.lines().count();

    if request.image_present {
        let image_path = request
            .image_path
            .as_deref()
            .unwrap_or("(unspecified image path)");
        if let Some(provider) = execution.provider.as_deref() {
            if let Some(image_path) = request.image_path.as_deref() {
                provider.validate()?;
                let image_base64 = encode_image_as_base64(image_path)?;
                let user_prompt = request
                    .query_text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| "Please analyze the attached image and respond concisely with the most relevant details for the runtime session.".to_string());
                let prompt = format!(
                    "You are Vela, a Rust-first agentic OS kernel runtime.\n\nSession: {} ({})\nMemory snapshot:\n{}{}\n\nLoaded skills: {}\nPending review candidates: {}\n\nUser image request:\n{}\n\nAttached image name: {}\n\nSupported runtime tools:\n- memory_snapshot\n- list_skills\n- view_memory (JSON: {{\"tool\":\"view_memory\",\"target\":\"memory\"}} or {{\"tool\":\"view_memory\",\"target\":\"user\"}})\n- search_session_history (JSON: {{\"tool\":\"search_session_history\",\"query\":\"keyword\",\"limit\":3}})\n- view_skill (JSON: {{\"tool\":\"view_skill\",\"name\":\"skill-name\"}})\nIf you need one tool before answering, respond with ONLY valid JSON for exactly one supported tool. Otherwise answer directly.",
                    session.title,
                    session.session_id,
                    memory,
                    compression_block,
                    skills.len(),
                    reviews.len(),
                    user_prompt,
                    std::path::Path::new(image_path).file_name().and_then(|n| n.to_str()).unwrap_or("attachment"),
                );
                return execute_provider_turn(
                    bootstrap,
                    session,
                    provider,
                    &prompt,
                    Some(vec![image_base64]),
                    &memory,
                    &skills,
                    lifecycle,
                );
            }
        }

        return Ok(RenderedChatResponse {
            content: Some(format!(
                "Vela executed a local image turn.\n\nImage: {}\nSession: {} ({})\nMemory snapshot lines: {}\nLoaded skills: {}\nPending review candidates: {}\n\nNo provider-backed image execution was available, so this deterministic local-kernel scaffold response kept persistence, review, and continuity live.",
                image_path,
                session.title,
                session.session_id,
                memory_lines,
                skills.len(),
                reviews.len(),
            )),
            source: "runtime-kernel",
            provider: execution.provider_label,
            model: execution.model,
        });
    }

    if let Some(query) = request.query_text.as_deref() {
        if let Some(provider) = execution.provider.as_deref() {
            provider.validate()?;
            let prompt = format!(
                "You are Vela, a Rust-first agentic OS kernel runtime.\n\nSession: {} ({})\nMemory snapshot:\n{}{}\n\nLoaded skills: {}\nPending review candidates: {}\n\nUser query:\n{}\n\nSupported runtime tools:\n- memory_snapshot\n- list_skills\n- view_memory (JSON: {{\"tool\":\"view_memory\",\"target\":\"memory\"}} or {{\"tool\":\"view_memory\",\"target\":\"user\"}})\n- search_session_history (JSON: {{\"tool\":\"search_session_history\",\"query\":\"keyword\",\"limit\":3}})\n- view_skill (JSON: {{\"tool\":\"view_skill\",\"name\":\"skill-name\"}})\nIf you need one tool before answering, respond with ONLY valid JSON for exactly one supported tool. Otherwise answer directly.",
                session.title,
                session.session_id,
                memory,
                compression_block,
                skills.len(),
                reviews.len(),
                query.trim(),
            );
            return execute_provider_turn(
                bootstrap, session, provider, &prompt, None, &memory, &skills, lifecycle,
            );
        }

        return Ok(RenderedChatResponse {
            content: Some(format!(
                "Vela executed a local kernel turn.\n\nQuery: {}\nSession: {} ({})\nMemory snapshot lines: {}\nLoaded skills: {}\nPending review candidates: {}\n\nNo provider-backed execution was available, so this deterministic local-kernel scaffold response kept persistence, review, and continuity live.",
                query.trim(),
                session.title,
                session.session_id,
                memory_lines,
                skills.len(),
                reviews.len(),
            )),
            source: "runtime-kernel",
            provider: None,
            model: None,
        });
    }

    if matches!(session.action, SessionAction::Created) {
        return Ok(RenderedChatResponse {
            content: Some(format!(
                "Interactive Vela runtime ready. Session: {} ({}). Loaded skills: {}. Pending review candidates: {}.",
                session.title,
                session.session_id,
                skills.len(),
                reviews.len(),
            )),
            source: "runtime-kernel",
            provider: execution.provider_label,
            model: execution.model,
        });
    }

    Ok(RenderedChatResponse {
        content: None,
        source: "runtime-kernel",
        provider: execution.provider_label,
        model: execution.model,
    })
}

fn resolve_runtime_execution(
    resolved: &ResolvedConfig,
    provider_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<RuntimeExecutionConfig> {
    let provider_label = provider_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .or_else(|| {
            resolved
                .runtime_provider
                .as_ref()
                .map(|s| s.trim().to_ascii_lowercase())
        });
    let model = model_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| resolved.runtime_model.clone());
    let provider = match provider_label.as_deref() {
        Some("ollama") => Some(Box::new(OllamaRuntimeProvider {
            label: "ollama".to_string(),
            model: model.clone(),
            base_url: resolved
                .runtime_ollama_base_url
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:11434".to_string()),
        }) as Box<dyn RuntimeProviderBackend>),
        Some(other) => bail!("unsupported runtime provider {other:?}"),
        None => None,
    };

    Ok(RuntimeExecutionConfig {
        provider,
        provider_label,
        model,
    })
}

/// Records one reflection attempt and returns either a retry prompt or a deterministic fallback.
fn handle_reflection_outcome(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    lifecycle: &mut RuntimeTurnRecorder,
    reflection_attempts: &mut usize,
    reason: &str,
    detail: serde_json::Value,
    fallback_message: &str,
    prompt_rewrite: String,
) -> Result<ReflectionOutcome> {
    *reflection_attempts += 1;
    let reflection_step = Some(*reflection_attempts);
    if *reflection_attempts > MAX_RUNTIME_REFLECTION_ATTEMPTS {
        lifecycle.record_phase(
            bootstrap,
            &session.session_id,
            "reflect",
            reflection_step,
            json!({"attempt": *reflection_attempts, "reason": reason, "detail": detail, "outcome": "fallback"}),
        )?;
        return Ok(ReflectionOutcome::Fallback(RenderedChatResponse {
            content: Some(fallback_message.to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
        }));
    }
    record_reflection_and_retry(
        bootstrap,
        session,
        lifecycle,
        *reflection_attempts,
        reflection_step,
        reason,
        detail,
    )?;
    Ok(ReflectionOutcome::RetryPrompt(prompt_rewrite))
}

/// Executes one provider-backed runtime turn and optionally completes a bounded local tool loop.
fn execute_provider_turn(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    provider: &dyn RuntimeProviderBackend,
    prompt: &str,
    images: Option<Vec<String>>,
    memory: &str,
    skills: &[vela_skills::SkillSummary],
    lifecycle: &mut RuntimeTurnRecorder,
) -> Result<RenderedChatResponse> {
    let mut current_prompt = prompt.to_string();
    let mut used_tool_loop = false;
    let mut reflection_attempts = 0usize;
    let mut tool_step = 0usize;

    while tool_step < MAX_RUNTIME_TOOL_STEPS {
        let response = provider.generate(&current_prompt, images.clone())?;
        match classify_provider_continuation(&response) {
            ProviderContinuation::ToolRequest(tool_request) => {
                tool_step += 1;
                used_tool_loop = true;
                persist_runtime_tool_request(
                    bootstrap,
                    &session.session_id,
                    &tool_request,
                    tool_step,
                )?;
                lifecycle.record_phase(
                    bootstrap,
                    &session.session_id,
                    "tool-request",
                    Some(tool_step),
                    json!({"request": tool_request.metadata_json(), "provider": provider.label(), "model": provider.model()}),
                )?;
                let tool_result = execute_runtime_tool(bootstrap, &tool_request, memory, skills);
                persist_runtime_tool_result(
                    bootstrap,
                    &session.session_id,
                    &tool_request,
                    tool_step,
                    &tool_result,
                )?;
                lifecycle.record_phase(
                    bootstrap,
                    &session.session_id,
                    "tool-result",
                    Some(tool_step),
                    json!({"request": tool_request.metadata_json(), "result_length": tool_result.len()}),
                )?;
                if tool_result.trim().is_empty() {
                    match handle_reflection_outcome(
                        bootstrap,
                        session,
                        lifecycle,
                        &mut reflection_attempts,
                        "empty-tool-result",
                        json!({"request": tool_request.metadata_json()}),
                        "Vela could not recover from an empty intermediate tool result within the bounded retry limit, so it fell back to a deterministic runtime response.",
                        format!(
                            "{}\n\nThe tool result for {} was empty and unusable. Do not repeat the same failed continuation blindly. Either request a supported tool with ONLY valid JSON for one approved tool, or answer directly.",
                            current_prompt,
                            tool_request.display_name(),
                        ),
                    )? {
                        ReflectionOutcome::Fallback(outcome) => return Ok(outcome),
                        ReflectionOutcome::RetryPrompt(prompt_rewrite) => {
                            current_prompt = prompt_rewrite;
                            continue;
                        }
                    }
                }

                let followup_instruction = if tool_step == MAX_RUNTIME_TOOL_STEPS {
                    "You have reached the maximum number of tool steps. Answer the user directly without requesting another tool."
                } else {
                    "You may either request another supported tool with ONLY valid JSON for one approved tool, or answer directly."
                };
                current_prompt = format!(
                    "{}\n\nCompleted tool step {} of {}.\nTool result for {}:\n{}\n\n{}",
                    current_prompt,
                    tool_step,
                    MAX_RUNTIME_TOOL_STEPS,
                    tool_request.request_text(),
                    tool_result,
                    followup_instruction,
                );
            }
            ProviderContinuation::FinalAnswer => {
                return Ok(RenderedChatResponse {
                    content: Some(response),
                    source: if used_tool_loop {
                        provider.tool_loop_response_source()
                    } else {
                        provider.direct_response_source()
                    },
                    provider: Some(provider.label().to_string()),
                    model: provider.model().map(str::to_string),
                });
            }
            ProviderContinuation::InvalidToolRequest => {
                match handle_reflection_outcome(
                    bootstrap,
                    session,
                    lifecycle,
                    &mut reflection_attempts,
                    "invalid-tool-request",
                    json!({"response": response}),
                    "Vela received an invalid provider continuation and exhausted the bounded reflection limit, so it fell back to a deterministic runtime response.",
                    format!(
                        "{}\n\nYour previous reply requested an unsupported or malformed tool envelope. Only these tools are allowed: memory_snapshot, list_skills, view_memory, search_session_history, view_skill. If you need one tool, respond with ONLY valid JSON for exactly one of those tool contracts. Otherwise answer the user directly.",
                        current_prompt,
                    ),
                )? {
                    ReflectionOutcome::Fallback(outcome) => return Ok(outcome),
                    ReflectionOutcome::RetryPrompt(prompt_rewrite) => current_prompt = prompt_rewrite,
                }
            }
            ProviderContinuation::EmptyResponse => {
                match handle_reflection_outcome(
                    bootstrap,
                    session,
                    lifecycle,
                    &mut reflection_attempts,
                    "empty-provider-response",
                    json!({}),
                    "Vela received an empty provider continuation and exhausted the bounded reflection limit, so it fell back to a deterministic runtime response.",
                    format!(
                        "{}\n\nYour previous reply was empty and unusable. Either request one supported tool with ONLY valid JSON for memory_snapshot, list_skills, view_memory, search_session_history, or view_skill, or answer the user directly with non-empty text.",
                        current_prompt,
                    ),
                )? {
                    ReflectionOutcome::Fallback(outcome) => return Ok(outcome),
                    ReflectionOutcome::RetryPrompt(prompt_rewrite) => current_prompt = prompt_rewrite,
                }
            }
        }
    }

    let final_response = provider.generate(&current_prompt, images)?;
    match classify_provider_continuation(&final_response) {
        ProviderContinuation::FinalAnswer => Ok(RenderedChatResponse {
            content: Some(final_response),
            source: provider.tool_loop_response_source(),
            provider: Some(provider.label().to_string()),
            model: provider.model().map(str::to_string),
        }),
        ProviderContinuation::ToolRequest(_) => Ok(RenderedChatResponse {
            content: Some("Vela reached the maximum bounded tool steps and fell back to a deterministic runtime response instead of continuing indefinitely.".to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
        }),
        ProviderContinuation::InvalidToolRequest => Ok(RenderedChatResponse {
            content: Some("Vela received an invalid provider continuation after the bounded tool loop and fell back to a deterministic runtime response.".to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
        }),
        ProviderContinuation::EmptyResponse => Ok(RenderedChatResponse {
            content: Some("Vela received an empty provider continuation after the bounded tool loop and fell back to a deterministic runtime response.".to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
        }),
    }
}

fn classify_provider_continuation(response: &str) -> ProviderContinuation {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return ProviderContinuation::EmptyResponse;
    }
    let json_body = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let looks_like_tool_envelope = json_body.starts_with('{') || trimmed.starts_with("```json");
    let Ok(request) = serde_json::from_str::<RuntimeToolRequest>(json_body) else {
        return if looks_like_tool_envelope {
            ProviderContinuation::InvalidToolRequest
        } else {
            ProviderContinuation::FinalAnswer
        };
    };
    let tool = match request.tool.trim() {
        "memory_snapshot" => RuntimeToolInvocation {
            name: RuntimeToolName::MemorySnapshot,
            target: None,
            query: None,
            skill_name: None,
            limit: None,
        },
        "list_skills" => RuntimeToolInvocation {
            name: RuntimeToolName::ListSkills,
            target: None,
            query: None,
            skill_name: None,
            limit: None,
        },
        "view_memory" => {
            let target = match request.target.as_deref() {
                Some(raw) => match vela_memory::MemoryTarget::parse(raw) {
                    Ok(target) => Some(target),
                    Err(_) => return ProviderContinuation::InvalidToolRequest,
                },
                None => Some(vela_memory::MemoryTarget::Memory),
            };
            RuntimeToolInvocation {
                name: RuntimeToolName::ViewMemory,
                target,
                query: None,
                skill_name: None,
                limit: None,
            }
        }
        "search_session_history" => {
            let query = request
                .query
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let Some(query) = query else {
                return ProviderContinuation::InvalidToolRequest;
            };
            RuntimeToolInvocation {
                name: RuntimeToolName::SearchSessionHistory,
                target: None,
                query: Some(query),
                skill_name: None,
                limit: request.limit.map(|value| value.clamp(1, 5)),
            }
        }
        "view_skill" => {
            let name = request
                .name
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let Some(skill_name) = name else {
                return ProviderContinuation::InvalidToolRequest;
            };
            RuntimeToolInvocation {
                name: RuntimeToolName::ViewSkill,
                target: None,
                query: None,
                skill_name: Some(skill_name),
                limit: None,
            }
        }
        _ => return ProviderContinuation::InvalidToolRequest,
    };
    ProviderContinuation::ToolRequest(tool)
}

fn record_reflection_and_retry(
    bootstrap: &BootstrapReport,
    session: &SessionRuntimeReport,
    lifecycle: &mut RuntimeTurnRecorder,
    attempt: usize,
    step: Option<usize>,
    reason: &str,
    detail: serde_json::Value,
) -> Result<()> {
    lifecycle.record_phase(
        bootstrap,
        &session.session_id,
        "reflect",
        step,
        json!({
            "attempt": attempt,
            "reason": reason,
            "detail": detail,
        }),
    )?;
    lifecycle.record_phase(
        bootstrap,
        &session.session_id,
        "retry",
        step,
        json!({
            "attempt": attempt,
            "reason": reason,
        }),
    )?;
    Ok(())
}

/// Executes one approved read-only runtime tool and returns its textual result.
fn execute_runtime_tool(
    bootstrap: &BootstrapReport,
    tool: &RuntimeToolInvocation,
    memory_snapshot: &str,
    skills: &[vela_skills::SkillSummary],
) -> String {
    match tool.name {
        RuntimeToolName::MemorySnapshot => memory_snapshot.to_string(),
        RuntimeToolName::ListSkills => {
            if skills.is_empty() {
                "(no loaded skills)".to_string()
            } else {
                skills
                    .iter()
                    .map(|skill| match skill.description.as_deref() {
                        Some(description) => format!("{} — {}", skill.name, description),
                        None => skill.name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        RuntimeToolName::ViewMemory => {
            let target = tool.target.unwrap_or(vela_memory::MemoryTarget::Memory);
            match vela_memory::view_memory(&bootstrap.vela_home, target) {
                Ok(view) => {
                    if view.entries.is_empty() {
                        format!("{}: (no entries)", target.label())
                    } else {
                        format!("{}:\n{}", target.label(), view.entries.join("\n\n"))
                    }
                }
                Err(error) => format!("failed to load {}: {}", target.label(), error),
            }
        }
        RuntimeToolName::SearchSessionHistory => {
            let query = tool.query.as_deref().unwrap_or_default();
            let limit = tool.limit.unwrap_or(3);
            match vela_state::search_session_history(
                &bootstrap.persistence.state_db_path,
                query,
                limit,
            ) {
                Ok(hits) if hits.is_empty() => {
                    format!("session search for {:?}: no matches", query)
                }
                Ok(hits) => hits
                    .into_iter()
                    .map(|hit| format!("{} :: {}", hit.session_title, hit.snippet))
                    .collect::<Vec<_>>()
                    .join("\n"),
                Err(error) => format!(
                    "failed to search session history for {:?}: {}",
                    query, error
                ),
            }
        }
        RuntimeToolName::ViewSkill => {
            let name = tool.skill_name.as_deref().unwrap_or_default();
            match vela_skills::view_skill(&bootstrap.vela_home, name) {
                Ok(skill) => format!("skill {}:\n{}", skill.name, skill.content),
                Err(error) => format!("failed to view skill {:?}: {}", name, error),
            }
        }
    }
}

/// Persists the requested runtime tool before execution begins.
fn persist_runtime_tool_request(
    bootstrap: &BootstrapReport,
    session_id: &str,
    tool: &RuntimeToolInvocation,
    step: usize,
) -> Result<()> {
    let metadata =
        json!({"source": "runtime-tool-loop", "step": step, "request": tool.metadata_json()})
            .to_string();
    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "runtime_tool_requested",
        metadata.clone(),
    )?;
    if !event_logged {
        bail!(
            "failed to persist runtime tool request event for session {:?}",
            session_id
        );
    }
    let message_logged = vela_state::append_message_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "tool-request",
        &tool.request_text(),
        Some(metadata),
    )?;
    if !message_logged {
        bail!(
            "failed to persist runtime tool request message for session {:?}",
            session_id
        );
    }
    Ok(())
}

/// Persists the completed runtime tool result and its metadata.
fn persist_runtime_tool_result(
    bootstrap: &BootstrapReport,
    session_id: &str,
    tool: &RuntimeToolInvocation,
    step: usize,
    result: &str,
) -> Result<()> {
    let metadata = json!({"source": "runtime-tool-loop", "step": step, "request": tool.metadata_json(), "result_length": result.len()}).to_string();
    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "runtime_tool_completed",
        metadata.clone(),
    )?;
    if !event_logged {
        bail!(
            "failed to persist runtime tool completion event for session {:?}",
            session_id
        );
    }
    let message_logged = vela_state::append_message_to_session(
        &bootstrap.persistence.state_db_path,
        session_id,
        "tool-result",
        result,
        Some(metadata),
    )?;
    if !message_logged {
        bail!(
            "failed to persist runtime tool result message for session {:?}",
            session_id
        );
    }
    Ok(())
}

fn call_ollama_generate(
    base_url: &str,
    model: &str,
    prompt: &str,
    images: Option<Vec<String>>,
) -> Result<String> {
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build Ollama HTTP client")?;
    let response = client
        .post(&url)
        .json(&OllamaGenerateRequest {
            model,
            prompt,
            stream: false,
            images,
        })
        .send()
        .with_context(|| format!("failed to call Ollama at {url}"))?
        .error_for_status()
        .with_context(|| format!("Ollama returned an error for {url}"))?;
    let payload: OllamaGenerateResponse = response
        .json()
        .context("failed to decode Ollama response")?;
    Ok(payload.response.trim().to_string())
}

fn encode_image_as_base64(path: &str) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read image attachment {:?}", path))?;
    Ok(BASE64_STANDARD.encode(bytes))
}

fn validate_ollama_base_url(base_url: &str) -> Result<()> {
    if std::env::var("VELA_ALLOW_REMOTE_OLLAMA")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
    {
        return Ok(());
    }

    let parsed = reqwest::Url::parse(base_url)
        .with_context(|| format!("invalid Ollama base URL {:?}", base_url))?;
    let host = parsed
        .host_str()
        .context("Ollama base URL is missing a host")?;
    let is_local = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|ip| {
                ip.is_loopback()
                    || ip == IpAddr::V4(Ipv4Addr::LOCALHOST)
                    || ip == IpAddr::V6(Ipv6Addr::LOCALHOST)
            })
            .unwrap_or(false);

    if !is_local {
        bail!(
            "refusing non-local Ollama endpoint {:?}; set VELA_ALLOW_REMOTE_OLLAMA=1 to opt in explicitly",
            base_url
        );
    }
    Ok(())
}

fn append_review_event(
    bootstrap: &BootstrapReport,
    session_id: Option<&str>,
    event_type: &str,
    payload_json: String,
) -> Result<()> {
    if let Some(session_id) = session_id {
        let logged = vela_state::append_event_to_session(
            &bootstrap.persistence.state_db_path,
            session_id,
            event_type,
            payload_json,
        )?;
        if !logged {
            tracing::warn!(%session_id, %event_type, "failed to append review event to originating session");
        }
    }
    Ok(())
}

fn load_scheduler_jobs(path: &std::path::Path) -> Result<Vec<ScheduledJob>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&content)?)
}

fn save_scheduler_jobs(path: &std::path::Path, jobs: &[ScheduledJob]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("scheduler jobs path has no parent directory"))?;
    let temp_path = parent.join(format!(
        "{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("jobs.json"),
        unix_timestamp_nanos()
    ));
    std::fs::write(&temp_path, serde_json::to_string_pretty(jobs)?)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

fn acquire_scheduler_jobs_lock(path: &std::path::Path) -> Result<SchedulerJobsLock> {
    let lock_path = path.with_extension("json.lock");
    for _ in 0..100 {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_) => {
                return Ok(SchedulerJobsLock { lock_path });
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => sleep(Duration::from_millis(25)),
            Err(err) => return Err(err.into()),
        }
    }
    bail!("timed out waiting for scheduler jobs lock")
}

struct SchedulerJobsLock {
    lock_path: std::path::PathBuf,
}

impl Drop for SchedulerJobsLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

fn normalize_scheduler_schedule(value: &str) -> Result<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        bail!("scheduler expression cannot be empty");
    }
    let fields = normalized.split_whitespace().collect::<Vec<_>>();
    let supported = matches!(
        fields.as_slice(),
        ["*", "*", "*", "*", "*"] | ["0", "*", "*", "*", "*"] | ["0", "0", "*", "*", "*"]
    ) || matches!(fields.as_slice(), [minute, "*", "*", "*", "*"] if minute.starts_with("*/") && minute[2..].parse::<u32>().ok().is_some_and(|value| value > 0));
    if !supported {
        bail!("unsupported scheduler expression {:?}; supported patterns are '* * * * *', '*/N * * * *', '0 * * * *', and '0 0 * * *'", normalized);
    }
    Ok(normalized)
}

fn normalize_scheduler_task(value: &str) -> Result<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        bail!("scheduled task cannot be empty");
    }
    Ok(normalized.to_string())
}

fn normalize_scheduler_source(source: Option<&str>) -> String {
    source
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("scheduler")
        .to_string()
}

fn validate_scheduler_job_id(id: &str) -> Result<&str> {
    let normalized = id.trim();
    if normalized.is_empty()
        || normalized == "."
        || normalized == ".."
        || normalized.contains('/')
        || normalized.contains('\\')
    {
        bail!("invalid scheduled job id");
    }
    Ok(normalized)
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn unix_timestamp_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Generates review candidates from the latest persisted session.
pub fn generate_review_candidates_from_latest_session(
    bootstrap: &BootstrapReport,
    limit: usize,
) -> Result<Option<vela_review::SuggestionReport>> {
    let Some(session) =
        vela_state::inspect_latest_session(&bootstrap.persistence.state_db_path, limit)?
    else {
        return Ok(None);
    };
    let report = vela_review::generate_candidates(
        &bootstrap.vela_home,
        vela_review::SuggestionInput {
            session_id: session.session_id.clone(),
            session_title: session.title.clone(),
            messages: session
                .messages
                .into_iter()
                .map(|m| vela_review::SuggestionMessage {
                    role: m.role,
                    content: m.content,
                })
                .collect(),
            events: session
                .events
                .into_iter()
                .map(|e| vela_review::SuggestionEvent {
                    event_type: e.event_type,
                    payload_json: e.payload_json,
                })
                .collect(),
        },
    )?;
    for candidate_id in &report.candidate_ids {
        let candidate = vela_review::get_candidate(&bootstrap.vela_home, candidate_id)?;
        append_review_event(
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
        )?;
    }
    let logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &report.session_id,
        "review_candidates_generated",
        json!({
            "session_id": report.session_id,
            "candidate_ids": report.candidate_ids,
            "skipped": report.skipped,
        })
        .to_string(),
    )?;
    if !logged {
        tracing::warn!("failed to append review_candidates_generated event to inspected session");
    }
    Ok(Some(report))
}

impl BootstrapReport {
    fn from_parts(
        config: BootstrapConfig,
        persistence: PersistenceReport,
        memory: MemoryReport,
        skills: SkillsReport,
        reviews: ReviewReport,
        extensions: ExtensionsReport,
    ) -> Self {
        Self {
            vela_home: config.vela_home,
            active_profile: config.active_profile,
            loaded_env_paths: config.loaded_env_paths,
            ignored_user_config: config.ignored_user_config,
            config_sources: config.config_sources,
            resolved_config: config.resolved_config,
            persistence,
            memory,
            skills,
            reviews,
            extensions,
        }
    }
}

pub use vela_memory::{MemoryTarget, MEMORY_CHAR_LIMIT, USER_CHAR_LIMIT};
pub use vela_state::SessionRequest;
