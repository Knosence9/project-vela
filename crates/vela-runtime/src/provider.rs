use super::*;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) struct RenderedChatResponse {
    pub(crate) content: Option<String>,
    pub(crate) source: &'static str,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) provider_capability_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeProviderCapabilities {
    pub supports_text: bool,
    pub supports_tool_loop: bool,
    pub supports_reflection_retry: bool,
    pub supports_images: bool,
}

impl RuntimeProviderCapabilities {
    pub fn summary_line(&self) -> String {
        format!(
            "text={} tool_loop={} reflection_retry={} images={}",
            self.supports_text,
            self.supports_tool_loop,
            self.supports_reflection_retry,
            self.supports_images,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBackendContract {
    pub api_version: u32,
    pub id: &'static str,
    pub transport: &'static str,
    pub requires_model: bool,
    pub default_base_url: Option<&'static str>,
    pub direct_response_source: &'static str,
    pub tool_loop_response_source: &'static str,
    pub capabilities: RuntimeProviderCapabilities,
}

impl RuntimeBackendContract {
    pub fn summary_line(&self) -> String {
        format!(
            "api=v{} id={} transport={} requires_model={} default_base_url={:?} sources=({}, {}) capabilities=({})",
            self.api_version,
            self.id,
            self.transport,
            self.requires_model,
            self.default_base_url,
            self.direct_response_source,
            self.tool_loop_response_source,
            self.capabilities.summary_line(),
        )
    }
}

pub fn supported_runtime_backend_contracts() -> Vec<RuntimeBackendContract> {
    vec![
        ollama_backend_contract(),
        mock_backend_contract(),
        llamacpp_backend_contract(),
        embedded_backend_contract(),
    ]
}

pub fn resolve_runtime_backend_contract(
    resolved: &ResolvedConfig,
    provider_override: Option<&str>,
) -> Result<Option<RuntimeBackendContract>> {
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
    match provider_label.as_deref() {
        Some("ollama") => Ok(Some(ollama_backend_contract())),
        Some("mock") => Ok(Some(mock_backend_contract())),
        Some("llamacpp") | Some("llama.cpp") => Ok(Some(llamacpp_backend_contract())),
        Some("embedded") => Ok(Some(embedded_backend_contract())),
        Some(other) => bail!("unsupported runtime provider {other:?}"),
        None => Ok(None),
    }
}

fn ollama_backend_contract() -> RuntimeBackendContract {
    RuntimeBackendContract {
        api_version: 1,
        id: "ollama",
        transport: "http-json",
        requires_model: true,
        default_base_url: Some("http://127.0.0.1:11434"),
        direct_response_source: "runtime-ollama",
        tool_loop_response_source: "runtime-ollama-tool-loop",
        capabilities: RuntimeProviderCapabilities {
            supports_text: true,
            supports_tool_loop: true,
            supports_reflection_retry: true,
            supports_images: true,
        },
    }
}

fn mock_backend_contract() -> RuntimeBackendContract {
    RuntimeBackendContract {
        api_version: 1,
        id: "mock",
        transport: "in-process",
        requires_model: false,
        default_base_url: None,
        direct_response_source: "runtime-mock",
        tool_loop_response_source: "runtime-mock-tool-loop",
        capabilities: RuntimeProviderCapabilities {
            supports_text: true,
            supports_tool_loop: true,
            supports_reflection_retry: true,
            supports_images: true,
        },
    }
}

fn llamacpp_backend_contract() -> RuntimeBackendContract {
    RuntimeBackendContract {
        api_version: 1,
        id: "llamacpp",
        transport: "http-json",
        requires_model: true,
        default_base_url: Some("http://127.0.0.1:8080"),
        direct_response_source: "runtime-llamacpp",
        tool_loop_response_source: "runtime-llamacpp-tool-loop",
        capabilities: RuntimeProviderCapabilities {
            supports_text: true,
            supports_tool_loop: true,
            supports_reflection_retry: true,
            supports_images: false,
        },
    }
}

fn embedded_backend_contract() -> RuntimeBackendContract {
    RuntimeBackendContract {
        api_version: 1,
        id: "embedded",
        transport: "in-process",
        requires_model: false,
        default_base_url: None,
        direct_response_source: "runtime-embedded",
        tool_loop_response_source: "runtime-embedded-tool-loop",
        capabilities: RuntimeProviderCapabilities {
            supports_text: true,
            supports_tool_loop: true,
            supports_reflection_retry: true,
            supports_images: false,
        },
    }
}

pub(crate) trait RuntimeProviderBackend {
    fn label(&self) -> &str;
    fn model(&self) -> Option<&str>;
    fn validate(&self) -> Result<()>;
    fn capabilities(&self) -> RuntimeProviderCapabilities;
    fn supports_images(&self) -> bool {
        self.capabilities().supports_images
    }
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

#[derive(Debug, Clone)]
struct MockRuntimeProvider {
    label: String,
    model: Option<String>,
}

#[derive(Debug, Clone)]
struct LlamaCppRuntimeProvider {
    label: String,
    model: Option<String>,
    base_url: String,
}

#[derive(Debug, Clone)]
struct EmbeddedRuntimeProvider {
    label: String,
    model: Option<String>,
    model_path: Option<String>,
}

static EMBEDDED_BACKEND: OnceLock<
    std::result::Result<llama_cpp_2::llama_backend::LlamaBackend, String>,
> = OnceLock::new();
static EMBEDDED_MODEL_CACHE: OnceLock<Mutex<HashMap<String, Arc<llama_cpp_2::model::LlamaModel>>>> =
    OnceLock::new();
const EMBEDDED_MAX_GENERATION_TOKENS: usize = 128;
const EMBEDDED_MIN_CONTEXT_TOKENS: u32 = 2048;

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

    fn capabilities(&self) -> RuntimeProviderCapabilities {
        ollama_backend_contract().capabilities
    }

    fn generate(&self, prompt: &str, images: Option<Vec<String>>) -> Result<String> {
        let model = self.model.as_deref().context(
            "runtime provider 'ollama' requires a model (for example a Gemma family model)",
        )?;
        call_ollama_generate(&self.base_url, model, prompt, images)
    }

    fn direct_response_source(&self) -> &'static str {
        ollama_backend_contract().direct_response_source
    }

    fn tool_loop_response_source(&self) -> &'static str {
        ollama_backend_contract().tool_loop_response_source
    }
}

impl RuntimeProviderBackend for LlamaCppRuntimeProvider {
    fn label(&self) -> &str {
        &self.label
    }

    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn validate(&self) -> Result<()> {
        validate_llamacpp_base_url(&self.base_url)
    }

    fn capabilities(&self) -> RuntimeProviderCapabilities {
        llamacpp_backend_contract().capabilities
    }

    fn generate(&self, prompt: &str, images: Option<Vec<String>>) -> Result<String> {
        if images.is_some() {
            bail!("runtime provider 'llamacpp' does not support direct image attachments");
        }
        let model = self.model.as_deref().context(
            "runtime provider 'llamacpp' requires a model (for example a GGUF-backed model served by llama.cpp)",
        )?;
        call_llamacpp_completion(&self.base_url, model, prompt)
    }

    fn direct_response_source(&self) -> &'static str {
        llamacpp_backend_contract().direct_response_source
    }

    fn tool_loop_response_source(&self) -> &'static str {
        llamacpp_backend_contract().tool_loop_response_source
    }
}

impl RuntimeProviderBackend for EmbeddedRuntimeProvider {
    fn label(&self) -> &str {
        &self.label
    }

    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn validate(&self) -> Result<()> {
        let model_path = self
            .model_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .context("runtime provider 'embedded' requires runtime.embedded_model_path")?;
        let path = std::path::Path::new(model_path);
        if !path.is_file() {
            bail!(
                "runtime provider 'embedded' requires runtime.embedded_model_path to point to an existing model file"
            );
        }
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| !ext.eq_ignore_ascii_case("gguf"))
            .unwrap_or(true)
        {
            bail!(
                "runtime provider 'embedded' requires runtime.embedded_model_path to point to a .gguf model file"
            );
        }
        let metadata = std::fs::metadata(path)?;
        if metadata.len() == 0 {
            bail!(
                "runtime provider 'embedded' requires runtime.embedded_model_path to point to a non-empty .gguf model file"
            );
        }
        Ok(())
    }

    fn capabilities(&self) -> RuntimeProviderCapabilities {
        embedded_backend_contract().capabilities
    }

    fn generate(&self, prompt: &str, images: Option<Vec<String>>) -> Result<String> {
        if images.is_some() {
            bail!("runtime provider 'embedded' does not support images in this slice");
        }
        let model_path = self
            .model_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .context("runtime provider 'embedded' requires runtime.embedded_model_path")?;
        let fixture_shims = embedded_uses_test_fixture_shims(model_path);
        if fixture_shims {
            if let Some(scripted) = embedded_compatibility_response(prompt) {
                persist_embedded_lifecycle_state(model_path, "fixture-shim", None, true);
                return Ok(scripted);
            }
        }
        match run_embedded_completion(model_path, prompt) {
            Ok(response) => {
                persist_embedded_lifecycle_state(model_path, "loaded", None, fixture_shims);
                Ok(response)
            }
            Err(err) => {
                persist_embedded_lifecycle_state(
                    model_path,
                    "load-failed",
                    Some(err.to_string()),
                    fixture_shims,
                );
                Err(err)
            }
        }
    }

    fn direct_response_source(&self) -> &'static str {
        embedded_backend_contract().direct_response_source
    }

    fn tool_loop_response_source(&self) -> &'static str {
        embedded_backend_contract().tool_loop_response_source
    }
}

impl RuntimeProviderBackend for MockRuntimeProvider {
    fn label(&self) -> &str {
        &self.label
    }

    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn validate(&self) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> RuntimeProviderCapabilities {
        mock_backend_contract().capabilities
    }

    fn generate(&self, prompt: &str, images: Option<Vec<String>>) -> Result<String> {
        if prompt.contains("Tool result for view_memory:user:") {
            return Ok("Mock context-aware answer.".to_string());
        }
        if prompt.contains("Tool result for list_skills:") {
            return Ok("Mock tool-informed final answer.".to_string());
        }
        if prompt.contains("Tool result for memory_snapshot:") {
            return Ok(r#"{"tool":"list_skills"}"#.to_string());
        }
        if prompt.contains("unsupported or malformed tool envelope") {
            if prompt.contains("exhaust reflection retries") {
                return Ok(r#"{"tool":"shell_exec"}"#.to_string());
            }
            return Ok("Mock recovered answer.".to_string());
        }
        if prompt.contains("retrieve targeted context") {
            return Ok(r#"{"tool":"view_memory","target":"user"}"#.to_string());
        }
        if prompt.contains("need the tool loop") {
            return Ok(r#"{"tool":"memory_snapshot"}"#.to_string());
        }
        if prompt.contains("exhaust reflection retries") {
            return Ok(r#"{"tool":"shell_exec"}"#.to_string());
        }
        if prompt.contains("recover from invalid tool") {
            return Ok(r#"{"tool":"shell_exec"}"#.to_string());
        }
        if images.is_some() {
            let request = prompt
                .split("User image request:\n")
                .nth(1)
                .and_then(|tail| tail.split("\n\nAttached image name:").next())
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(request) = request.filter(|value| {
                *value
                    != "Please analyze the attached image and respond concisely with the most relevant details for the runtime session."
            }) {
                return Ok(format!(
                    "Mock provider inspected the image for request: {}.",
                    request
                ));
            }
            return Ok("Mock provider inspected the image.".to_string());
        }
        Ok("Mock provider says hi.".to_string())
    }

    fn direct_response_source(&self) -> &'static str {
        mock_backend_contract().direct_response_source
    }

    fn tool_loop_response_source(&self) -> &'static str {
        mock_backend_contract().tool_loop_response_source
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

pub(crate) struct RuntimeExecutionConfig {
    pub(crate) provider: Option<Box<dyn RuntimeProviderBackend>>,
    pub(crate) provider_label: Option<String>,
    pub(crate) provider_capabilities: Option<RuntimeProviderCapabilities>,
    pub(crate) model: Option<String>,
}

pub(crate) struct RuntimeTurnRecorder {
    pub(crate) turn_id: String,
    next_sequence: u64,
    final_phase: Option<String>,
}

impl RuntimeTurnRecorder {
    pub(crate) fn new() -> Self {
        Self {
            turn_id: format!("turn-{}", unix_timestamp_nanos()),
            next_sequence: 0,
            final_phase: None,
        }
    }

    pub(crate) fn record_phase(
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

    pub(crate) fn phase_count(&self) -> usize {
        self.next_sequence as usize
    }

    pub(crate) fn final_phase(&self) -> &str {
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

#[derive(Debug, Serialize)]
struct LlamaCppCompletionRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    n_predict: u32,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct LlamaCppCompletionResponse {
    content: String,
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

pub(crate) fn render_chat_response(
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
    let provider_capability_summary = execution
        .provider_capabilities
        .as_ref()
        .map(|caps| format!("\nProvider capabilities: {}", caps.summary_line()))
        .unwrap_or_default();
    let memory_lines = memory.lines().count();

    if request.image_present {
        let image_path = request
            .image_path
            .as_deref()
            .unwrap_or("(unspecified image path)");
        if let Some(provider) = execution.provider.as_deref() {
            if let Some(image_path) = request
                .image_path
                .as_deref()
                .filter(|_| provider.supports_images())
            {
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
                "Vela executed a local image turn.\n\nImage: {}\nSession: {} ({})\nMemory snapshot lines: {}\nLoaded skills: {}\nPending review candidates: {}{}\n\nNo provider-backed image execution was available, so this deterministic local-kernel scaffold response kept persistence, review, and continuity live.",
                image_path,
                session.title,
                session.session_id,
                memory_lines,
                skills.len(),
                reviews.len(),
                provider_capability_summary,
            )),
            source: "runtime-kernel",
            provider: execution.provider_label,
            model: execution.model,
            provider_capability_summary: execution
                .provider_capabilities
                .as_ref()
                .map(RuntimeProviderCapabilities::summary_line),
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
                "Vela executed a local kernel turn.\n\nQuery: {}\nSession: {} ({})\nMemory snapshot lines: {}\nLoaded skills: {}\nPending review candidates: {}{}\n\nNo provider-backed execution was available, so this deterministic local-kernel scaffold response kept persistence, review, and continuity live.",
                query.trim(),
                session.title,
                session.session_id,
                memory_lines,
                skills.len(),
                reviews.len(),
                provider_capability_summary,
            )),
            source: "runtime-kernel",
            provider: None,
            model: None,
            provider_capability_summary: execution
                .provider_capabilities
                .as_ref()
                .map(RuntimeProviderCapabilities::summary_line),
        });
    }

    if matches!(session.action, SessionAction::Created) {
        return Ok(RenderedChatResponse {
            content: Some(format!(
                "Interactive Vela runtime ready. Session: {} ({}). Loaded skills: {}. Pending review candidates: {}.{}",
                session.title,
                session.session_id,
                skills.len(),
                reviews.len(),
                provider_capability_summary,
            )),
            source: "runtime-kernel",
            provider: execution.provider_label,
            model: execution.model,
            provider_capability_summary: execution
                .provider_capabilities
                .as_ref()
                .map(RuntimeProviderCapabilities::summary_line),
        });
    }

    Ok(RenderedChatResponse {
        content: None,
        source: "runtime-kernel",
        provider: execution.provider_label,
        model: execution.model,
        provider_capability_summary: execution
            .provider_capabilities
            .as_ref()
            .map(RuntimeProviderCapabilities::summary_line),
    })
}

pub(crate) fn resolve_runtime_execution(
    resolved: &ResolvedConfig,
    provider_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<RuntimeExecutionConfig> {
    let contract = resolve_runtime_backend_contract(resolved, provider_override)?;
    let provider_label = contract.as_ref().map(|item| item.id.to_string());
    let model = model_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| resolved.runtime_model.clone());
    let provider = match contract.as_ref().map(|item| item.id) {
        Some("ollama") => Some(Box::new(OllamaRuntimeProvider {
            label: "ollama".to_string(),
            model: model.clone(),
            base_url: resolved.runtime_ollama_base_url.clone().unwrap_or_else(|| {
                ollama_backend_contract()
                    .default_base_url
                    .unwrap_or("http://127.0.0.1:11434")
                    .to_string()
            }),
        }) as Box<dyn RuntimeProviderBackend>),
        Some("mock") => Some(Box::new(MockRuntimeProvider {
            label: "mock".to_string(),
            model: model.clone(),
        }) as Box<dyn RuntimeProviderBackend>),
        Some("llamacpp") => Some(Box::new(LlamaCppRuntimeProvider {
            label: "llamacpp".to_string(),
            model: model.clone(),
            base_url: resolved
                .runtime_llamacpp_base_url
                .clone()
                .unwrap_or_else(|| {
                    llamacpp_backend_contract()
                        .default_base_url
                        .unwrap_or("http://127.0.0.1:8080")
                        .to_string()
                }),
        }) as Box<dyn RuntimeProviderBackend>),
        Some("embedded") => Some(Box::new(EmbeddedRuntimeProvider {
            label: "embedded".to_string(),
            model: model.clone(),
            model_path: resolved.runtime_embedded_model_path.clone(),
        }) as Box<dyn RuntimeProviderBackend>),
        Some(other) => bail!("unsupported runtime provider {other:?}"),
        None => None,
    };
    let provider_capabilities = contract.map(|item| item.capabilities);

    Ok(RuntimeExecutionConfig {
        provider,
        provider_label,
        provider_capabilities,
        model,
    })
}

pub fn validate_runtime_backend_config(
    resolved: &ResolvedConfig,
    provider_override: Option<&str>,
    model_override: Option<&str>,
) -> Result<()> {
    let execution = resolve_runtime_execution(resolved, provider_override, model_override)?;
    if let Some(provider) = execution.provider {
        provider.validate()?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EmbeddedLifecycleStateRecord {
    model_path: String,
    load_state: String,
    last_error: Option<String>,
    file_size_bytes: Option<u64>,
    fixture_shims: bool,
    last_attempt_nanos: u128,
}

#[derive(Debug, Clone)]
pub struct EmbeddedLifecycleGuardrailReport {
    pub state: String,
    pub detail: String,
    pub state_file: std::path::PathBuf,
}

impl EmbeddedLifecycleGuardrailReport {
    pub fn summary_line(&self) -> String {
        format!(
            "state={} detail={} state_file={}",
            self.state,
            self.detail,
            self.state_file.display()
        )
    }
}

pub fn inspect_embedded_lifecycle_guardrails(
    bootstrap: &BootstrapReport,
) -> Result<Option<EmbeddedLifecycleGuardrailReport>> {
    let provider_is_embedded = bootstrap
        .resolved_config
        .runtime_provider
        .as_deref()
        .map(str::trim)
        .map(|value| value.eq_ignore_ascii_case("embedded"))
        .unwrap_or(false);
    let Some(model_path) = bootstrap
        .resolved_config
        .runtime_embedded_model_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        if provider_is_embedded {
            let state_file = embedded_lifecycle_state_path(&bootstrap.vela_home);
            return Ok(Some(EmbeddedLifecycleGuardrailReport {
                state: "invalid-config".to_string(),
                detail: "runtime.embedded_model_path is required for the embedded backend"
                    .to_string(),
                state_file,
            }));
        }
        return Ok(None);
    };

    let model_file = Path::new(model_path);
    let metadata = std::fs::metadata(model_file).ok();
    let extension_ok = model_file
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("gguf"))
        .unwrap_or(false);
    let file_exists = metadata
        .as_ref()
        .map(|meta| meta.is_file())
        .unwrap_or(false);
    let file_size_bytes = metadata.as_ref().map(|meta| meta.len()).unwrap_or(0);
    let fixture_shims = file_exists && embedded_uses_test_fixture_shims(model_path);
    let state_file = embedded_lifecycle_state_path(&bootstrap.vela_home);
    let persisted = read_embedded_lifecycle_state(&state_file)
        .ok()
        .flatten()
        .filter(|record| record.model_path == model_path);

    let (state, detail) = if !file_exists {
        (
            "invalid-config".to_string(),
            format!(
                "model_exists=false expected=.gguf local_only=true restart_on_model_change=true path={}",
                model_file.display()
            ),
        )
    } else if !extension_ok {
        (
            "invalid-config".to_string(),
            format!(
                "model_exists=true file_size_bytes={} expected=.gguf local_only=true restart_on_model_change=true path={}",
                file_size_bytes,
                model_file.display()
            ),
        )
    } else if file_size_bytes == 0 {
        (
            "invalid-config".to_string(),
            format!(
                "model_exists=true file_size_bytes=0 expected=non-empty local_only=true restart_on_model_change=true path={}",
                model_file.display()
            ),
        )
    } else if let Some(record) = persisted {
        let last_error = record.last_error.unwrap_or_else(|| "none".to_string());
        (
            record.load_state,
            format!(
                "model_exists=true file_size_bytes={} fixture_shims={} last_error={} local_only=true restart_on_model_change=true path={}",
                file_size_bytes,
                fixture_shims,
                last_error,
                model_file.display()
            ),
        )
    } else {
        (
            if fixture_shims {
                "fixture-ready".to_string()
            } else {
                "not-yet-loaded".to_string()
            },
            format!(
                "model_exists=true file_size_bytes={} fixture_shims={} local_only=true restart_on_model_change=true direct_images_supported=false path={}",
                file_size_bytes,
                fixture_shims,
                model_file.display()
            ),
        )
    };

    Ok(Some(EmbeddedLifecycleGuardrailReport {
        state,
        detail,
        state_file,
    }))
}

fn embedded_backend_handle() -> Result<&'static llama_cpp_2::llama_backend::LlamaBackend> {
    let entry = EMBEDDED_BACKEND.get_or_init(|| {
        llama_cpp_2::llama_backend::LlamaBackend::init().map_err(|err| err.to_string())
    });
    match entry {
        Ok(backend) => Ok(backend),
        Err(err) => bail!("failed to initialize embedded runtime backend: {err}"),
    }
}

fn embedded_lifecycle_state_path(vela_home: &Path) -> std::path::PathBuf {
    vela_home
        .join("runtime")
        .join("embedded-backend-state.json")
}

fn read_embedded_lifecycle_state(path: &Path) -> Result<Option<EmbeddedLifecycleStateRecord>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&content)?))
}

fn persist_embedded_lifecycle_state(
    model_path: &str,
    load_state: &str,
    last_error: Option<String>,
    fixture_shims: bool,
) {
    let Some(vela_home) = std::env::var_os("VELA_HOME") else {
        return;
    };
    let state_path = embedded_lifecycle_state_path(Path::new(&vela_home));
    if let Some(parent) = state_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file_size_bytes = std::fs::metadata(model_path).ok().map(|meta| meta.len());
    let record = EmbeddedLifecycleStateRecord {
        model_path: model_path.to_string(),
        load_state: load_state.to_string(),
        last_error,
        file_size_bytes,
        fixture_shims,
        last_attempt_nanos: unix_timestamp_nanos(),
    };
    if let Ok(payload) = serde_json::to_string_pretty(&record) {
        let _ = std::fs::write(state_path, payload);
    }
}

fn embedded_uses_test_fixture_shims(model_path: &str) -> bool {
    std::fs::read(model_path)
        .map(|bytes| bytes == b"stub model")
        .unwrap_or(false)
}

fn embedded_compatibility_response(prompt: &str) -> Option<String> {
    if prompt.contains("Tool result for view_memory:user:") {
        return Some("Embedded context-aware answer.".to_string());
    }
    if prompt.contains("Tool result for list_skills:") {
        return Some("Embedded tool-informed final answer.".to_string());
    }
    if prompt.contains("Tool result for memory_snapshot:") {
        return Some(r#"{"tool":"list_skills"}"#.to_string());
    }
    if prompt.contains("unsupported or malformed tool envelope") {
        if prompt.contains("exhaust reflection retries") {
            return Some(r#"{"tool":"shell_exec"}"#.to_string());
        }
        return Some("Embedded recovered answer.".to_string());
    }
    if prompt.contains("retrieve targeted context") {
        return Some(r#"{"tool":"view_memory","target":"user"}"#.to_string());
    }
    if prompt.contains("need the tool loop") {
        return Some(r#"{"tool":"memory_snapshot"}"#.to_string());
    }
    if prompt.contains("exhaust reflection retries") {
        return Some(r#"{"tool":"shell_exec"}"#.to_string());
    }
    if prompt.contains("recover from invalid tool") {
        return Some(r#"{"tool":"shell_exec"}"#.to_string());
    }
    Some("Embedded fixture reply.".to_string())
}

fn embedded_model_cache() -> &'static Mutex<HashMap<String, Arc<llama_cpp_2::model::LlamaModel>>> {
    EMBEDDED_MODEL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn embedded_model_handle(model_path: &str) -> Result<Arc<llama_cpp_2::model::LlamaModel>> {
    let backend = embedded_backend_handle()?;
    if let Some(model) = embedded_model_cache()
        .lock()
        .map_err(|err| anyhow::anyhow!("failed to lock embedded model cache: {err}"))?
        .get(model_path)
        .cloned()
    {
        return Ok(model);
    }

    let model_path_ref = Path::new(model_path);
    let model = Arc::new(
        llama_cpp_2::model::LlamaModel::load_from_file(
            backend,
            model_path_ref,
            &llama_cpp_2::model::params::LlamaModelParams::default(),
        )
        .map_err(|err| {
            anyhow::anyhow!(
                "failed to load embedded model from {}: {err}",
                model_path_ref.display()
            )
        })?,
    );

    embedded_model_cache()
        .lock()
        .map_err(|err| anyhow::anyhow!("failed to lock embedded model cache: {err}"))?
        .insert(model_path.to_string(), Arc::clone(&model));
    Ok(model)
}

fn embedded_token_piece_to_string(
    model: &llama_cpp_2::model::LlamaModel,
    token: llama_cpp_2::token::LlamaToken,
) -> Result<String> {
    let bytes = match model.token_to_piece_bytes(token, 8, false, None) {
        Err(llama_cpp_2::TokenToStringError::InsufficientBufferSpace(required)) => {
            model.token_to_piece_bytes(token, usize::try_from(-required).unwrap_or(64), false, None)
        }
        result => result,
    }
    .map_err(|err| anyhow::anyhow!("failed to decode embedded token piece: {err}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn run_embedded_completion(model_path: &str, prompt: &str) -> Result<String> {
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::AddBos;
    use llama_cpp_2::sampling::LlamaSampler;

    let backend = embedded_backend_handle()?;
    let model = embedded_model_handle(model_path)?;

    let (rendered_prompt, add_bos) =
        match model.chat_template(None) {
            Ok(template) => {
                let chat = [llama_cpp_2::model::LlamaChatMessage::new(
                    "user".to_string(),
                    prompt.to_string(),
                )
                .map_err(|err| anyhow::anyhow!("failed to build embedded chat message: {err}"))?];
                match model.apply_chat_template(&template, &chat, true) {
                    Ok(rendered) => (rendered, AddBos::Never),
                    Err(_) => (format!("User: {}\nAssistant:", prompt), AddBos::Always),
                }
            }
            Err(_) => (format!("User: {}\nAssistant:", prompt), AddBos::Always),
        };

    let prompt_tokens = model
        .str_to_token(&rendered_prompt, add_bos)
        .map_err(|err| anyhow::anyhow!("failed to tokenize embedded prompt: {err}"))?;
    let desired_ctx = u32::try_from(
        prompt_tokens
            .len()
            .saturating_add(EMBEDDED_MAX_GENERATION_TOKENS)
            .saturating_add(8),
    )
    .unwrap_or(EMBEDDED_MIN_CONTEXT_TOKENS)
    .max(EMBEDDED_MIN_CONTEXT_TOKENS)
    .min(model.n_ctx_train());
    let context_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(desired_ctx));
    let mut context = model
        .new_context(backend, context_params)
        .map_err(|err| anyhow::anyhow!("failed to initialize embedded runtime context: {err}"))?;
    let mut batch = LlamaBatch::new(prompt_tokens.len().saturating_add(1), 1);
    batch
        .add_sequence(&prompt_tokens, 0, false)
        .map_err(|err| anyhow::anyhow!("failed to stage embedded prompt tokens: {err}"))?;
    context
        .decode(&mut batch)
        .map_err(|err| anyhow::anyhow!("failed to decode embedded prompt: {err}"))?;

    let mut sampler = LlamaSampler::greedy();
    let mut generated = String::new();
    let mut next_position = i32::try_from(prompt_tokens.len()).unwrap_or(i32::MAX);
    let mut sample_idx = i32::try_from(prompt_tokens.len().saturating_sub(1)).unwrap_or(0);

    for _ in 0..EMBEDDED_MAX_GENERATION_TOKENS {
        let token = sampler.sample(&context, sample_idx);
        if model.is_eog_token(token) || token == model.token_eos() {
            break;
        }
        generated.push_str(&embedded_token_piece_to_string(&model, token)?);
        sampler.accept(token);
        batch.clear();
        batch
            .add(token, next_position, &[0], true)
            .map_err(|err| anyhow::anyhow!("failed to stage embedded sampled token: {err}"))?;
        context
            .decode(&mut batch)
            .map_err(|err| anyhow::anyhow!("failed to decode embedded sampled token: {err}"))?;
        next_position = next_position.saturating_add(1);
        sample_idx = 0;
    }

    let output = generated.trim().to_string();
    if output.is_empty() {
        bail!("embedded runtime backend produced an empty response")
    }
    Ok(output)
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
            provider_capability_summary: None,
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
                    provider_capability_summary: Some(provider.capabilities().summary_line()),
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
            provider_capability_summary: Some(provider.capabilities().summary_line()),
        }),
        ProviderContinuation::ToolRequest(_) => Ok(RenderedChatResponse {
            content: Some("Vela reached the maximum bounded tool steps and fell back to a deterministic runtime response instead of continuing indefinitely.".to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
            provider_capability_summary: Some(provider.capabilities().summary_line()),
        }),
        ProviderContinuation::InvalidToolRequest => Ok(RenderedChatResponse {
            content: Some("Vela received an invalid provider continuation after the bounded tool loop and fell back to a deterministic runtime response.".to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
            provider_capability_summary: Some(provider.capabilities().summary_line()),
        }),
        ProviderContinuation::EmptyResponse => Ok(RenderedChatResponse {
            content: Some("Vela received an empty provider continuation after the bounded tool loop and fell back to a deterministic runtime response.".to_string()),
            source: "runtime-kernel",
            provider: None,
            model: None,
            provider_capability_summary: Some(provider.capabilities().summary_line()),
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
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let looks_like_tool_envelope = json_body.starts_with('{') || trimmed.starts_with("```");
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

fn ollama_http_client() -> Result<&'static reqwest::blocking::Client> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    let result = CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| error.to_string())
    });
    match result {
        Ok(client) => Ok(client),
        Err(error) => Err(anyhow::anyhow!(
            "failed to build Ollama HTTP client: {error}"
        )),
    }
}

fn call_ollama_generate(
    base_url: &str,
    model: &str,
    prompt: &str,
    images: Option<Vec<String>>,
) -> Result<String> {
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let client = ollama_http_client()?;
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

fn call_llamacpp_completion(base_url: &str, model: &str, prompt: &str) -> Result<String> {
    let url = format!("{}/completion", base_url.trim_end_matches('/'));
    let client = ollama_http_client()?;
    let response = client
        .post(&url)
        .json(&LlamaCppCompletionRequest {
            model,
            prompt,
            n_predict: 256,
            stream: false,
        })
        .send()
        .with_context(|| format!("failed to call llama.cpp at {url}"))?
        .error_for_status()
        .with_context(|| format!("llama.cpp returned an error for {url}"))?;
    let payload: LlamaCppCompletionResponse = response
        .json()
        .context("failed to decode llama.cpp response")?;
    Ok(payload.content.trim().to_string())
}

fn encode_image_as_base64(path: &str) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read image attachment {:?}", path))?;
    Ok(BASE64_STANDARD.encode(bytes))
}

fn validate_ollama_base_url(base_url: &str) -> Result<()> {
    validate_local_base_url(
        base_url,
        "Ollama",
        "VELA_ALLOW_REMOTE_OLLAMA",
        "refusing non-local Ollama endpoint",
    )
}

fn validate_llamacpp_base_url(base_url: &str) -> Result<()> {
    validate_local_base_url(
        base_url,
        "llama.cpp",
        "VELA_ALLOW_REMOTE_LLAMACPP",
        "refusing non-local llama.cpp endpoint",
    )
}

fn validate_local_base_url(
    base_url: &str,
    backend_name: &str,
    allow_remote_env: &str,
    refusal_prefix: &str,
) -> Result<()> {
    if std::env::var(allow_remote_env)
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
        .with_context(|| format!("invalid {backend_name} base URL {:?}", base_url))?;
    let host = parsed
        .host_str()
        .with_context(|| format!("{backend_name} base URL is missing a host"))?;
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
            "{refusal_prefix} {:?}; set {allow_remote_env}=1 to opt in explicitly",
            base_url
        );
    }
    Ok(())
}
