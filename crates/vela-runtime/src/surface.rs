use super::*;

#[derive(Debug, Clone)]
/// Describes the durable gateway paths ensured during bootstrap.
pub struct GatewaySetupReport {
    pub gateway_dir: std::path::PathBuf,
    pub config_path: std::path::PathBuf,
    pub inbox_dir: std::path::PathBuf,
    pub outbox_dir: std::path::PathBuf,
    pub config_existed_before: bool,
}

#[derive(Debug, Clone)]
/// Captures gateway setup data plus the resolved runtime session.
pub struct GatewayStartReport {
    pub setup: GatewaySetupReport,
    pub session: SessionRuntimeReport,
}

#[derive(Debug, Clone)]
/// Captures one externally delivered gateway webhook plus its durable outbox record.
pub struct GatewayWebhookDeliveryReport {
    pub setup: GatewaySetupReport,
    pub session: SessionRuntimeReport,
    pub event_type: String,
    pub url: String,
    pub outbox_record_path: std::path::PathBuf,
    pub status_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents one durable bounded subagent delegation request.
pub struct SubagentDelegationRecord {
    pub id: String,
    pub role: String,
    pub task: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub session_id: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
/// Describes the durable subagent delegation files ensured during bootstrap.
pub struct SubagentDelegationSetupReport {
    pub agents_dir: std::path::PathBuf,
    pub delegations_path: std::path::PathBuf,
    pub delegations_existed_before: bool,
    pub delegation_count: usize,
}

#[derive(Debug, Clone)]
/// Captures one durable subagent delegation request plus the resolved command session.
pub struct SubagentDelegationRequestReport {
    pub setup: SubagentDelegationSetupReport,
    pub session: SessionRuntimeReport,
    pub record: SubagentDelegationRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents one durable bounded MCP bridge request.
pub struct McpBridgeCallRecord {
    pub id: String,
    pub server: String,
    pub tool: String,
    pub payload: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub session_id: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
/// Describes the durable MCP bridge files ensured during bootstrap.
pub struct McpBridgeSetupReport {
    pub mcp_dir: std::path::PathBuf,
    pub requests_path: std::path::PathBuf,
    pub requests_existed_before: bool,
    pub request_count: usize,
}

#[derive(Debug, Clone)]
/// Captures one durable MCP bridge request plus the resolved command session.
pub struct McpBridgeRequestReport {
    pub setup: McpBridgeSetupReport,
    pub session: SessionRuntimeReport,
    pub record: McpBridgeCallRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents the durable model-lab policy that governs deeper experimentation.
pub struct ModelLabPolicyRecord {
    pub version: u32,
    pub summary: String,
    pub graduation_gates: Vec<String>,
    pub allowed_experiment_strategies: Vec<String>,
    pub prohibited_behaviors: Vec<String>,
    pub required_evidence: Vec<String>,
    #[serde(default)]
    pub adapter_finetune_intake_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents one bounded architecture experiment slot that can drive future routing work.
pub struct BackendExperimentSlotRecord {
    pub id: String,
    pub status: String,
    pub strategy: String,
    pub summary: Option<String>,
    pub hypothesis: Option<String>,
    pub default_prompt: String,
    pub allowed_backends: Vec<String>,
    #[serde(default)]
    pub unchanged_surfaces: Vec<String>,
    #[serde(default)]
    pub rollback_note: Option<String>,
    #[serde(default)]
    pub promotion_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents one backend-specific result captured by the eval harness.
pub struct BackendEvalResultRecord {
    pub backend_id: String,
    pub transport: String,
    pub status: String,
    pub duration_ms: u64,
    pub response_source: Option<String>,
    pub response_model: Option<String>,
    pub provider_capabilities: Option<String>,
    pub response_chars: usize,
    pub response_preview: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents one durable backend eval run stored in `runs.json`.
pub struct BackendEvalRunRecord {
    pub id: String,
    pub prompt: String,
    pub backends: Vec<String>,
    pub created_at: i64,
    pub session_id: String,
    pub experiment_slot: Option<String>,
    pub model_override: Option<String>,
    #[serde(default)]
    pub parity_summary: Option<String>,
    #[serde(default)]
    pub score_summary: Option<String>,
    pub results: Vec<BackendEvalResultRecord>,
}

#[derive(Debug, Clone)]
/// Describes the durable backend eval files ensured during bootstrap.
pub struct BackendEvalSetupReport {
    pub evals_dir: std::path::PathBuf,
    pub runs_path: std::path::PathBuf,
    pub slots_path: std::path::PathBuf,
    pub policy_path: std::path::PathBuf,
    pub runs_existed_before: bool,
    pub slots_existed_before: bool,
    pub policy_existed_before: bool,
    pub run_count: usize,
    pub slot_count: usize,
}

#[derive(Debug, Clone)]
/// Captures one backend eval run plus the resolved command session.
pub struct BackendEvalRunReport {
    pub setup: BackendEvalSetupReport,
    pub session: SessionRuntimeReport,
    pub record: BackendEvalRunRecord,
}

#[derive(Debug, Clone)]
/// Enriches a published experiment slot with the latest durable eval evidence for operators.
pub struct BackendExperimentSlotInspection {
    pub slot: BackendExperimentSlotRecord,
    pub latest_eval_id: Option<String>,
    pub latest_eval_created_at: Option<i64>,
    pub latest_eval_parity_summary: Option<String>,
    pub latest_eval_score_summary: Option<String>,
    pub latest_eval_backends: Vec<String>,
    pub latest_eval_passed_backends: Vec<String>,
    pub latest_eval_failed_backends: Vec<String>,
    pub latest_eval_capability_groups: Vec<String>,
    pub latest_eval_result_count: usize,
    pub latest_backend_evidence: Vec<String>,
}

#[derive(Debug, Clone)]
/// Describes the durable scheduler files ensured during bootstrap.
pub struct SchedulerSetupReport {
    pub scheduler_dir: std::path::PathBuf,
    pub config_path: std::path::PathBuf,
    pub jobs_path: std::path::PathBuf,
    pub config_existed_before: bool,
    pub jobs_existed_before: bool,
    pub job_count: usize,
}

#[derive(Debug, Clone)]
/// Captures scheduler setup data plus the resolved runtime session.
pub struct SchedulerStartReport {
    pub setup: SchedulerSetupReport,
    pub session: SessionRuntimeReport,
    pub executed_job_count: usize,
    pub recovered_job_count: usize,
    pub failed_job_count: usize,
}

#[derive(Debug, Clone)]
/// Captures one executed chat/runtime turn plus any operator-triggered review work.
pub struct ChatTurnReport {
    pub session: SessionRuntimeReport,
    pub turn_id: String,
    pub response: Option<String>,
    pub response_source: String,
    pub response_provider: Option<String>,
    pub response_model: Option<String>,
    pub response_provider_capabilities: Option<String>,
    pub lifecycle_phase_count: usize,
    pub final_phase: String,
    pub emitted_signal_count: usize,
    pub generated_candidate_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents one durable scheduler registration stored in `jobs.json`.
pub struct ScheduledJob {
    pub id: String,
    pub schedule: String,
    pub task: String,
    pub source: String,
    pub status: String,
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub next_run_at: i64,
    #[serde(default)]
    pub last_started_at: Option<i64>,
    #[serde(default)]
    pub last_completed_at: Option<i64>,
    #[serde(default)]
    pub last_failed_at: Option<i64>,
    #[serde(default)]
    pub last_recovered_at: Option<i64>,
    #[serde(default)]
    pub last_outcome: Option<String>,
    #[serde(default)]
    pub last_progression: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub run_count: u64,
    #[serde(default)]
    pub recovery_count: u64,
    #[serde(default)]
    pub missed_run_count: u64,
    #[serde(default)]
    pub last_missed_run_count: u64,
    #[serde(default)]
    pub last_session_id: Option<String>,
    #[serde(default)]
    pub execution_token: Option<String>,
    #[serde(default)]
    pub lease_expires_at: Option<i64>,
    #[serde(default)]
    pub delivery_webhook_url: Option<String>,
    #[serde(default)]
    pub delivery_event_type: Option<String>,
    #[serde(default)]
    pub last_delivery_at: Option<i64>,
    #[serde(default)]
    pub last_delivery_outcome: Option<String>,
    #[serde(default)]
    pub last_delivery_progression: Option<String>,
    #[serde(default)]
    pub delivery_attempt_count: u64,
    #[serde(default)]
    pub last_delivery_status_code: Option<u16>,
    #[serde(default)]
    pub last_delivery_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Describes one kernel-owned runtime config drift detected during extension reload.
pub struct RestartRequiredRuntimeDrift {
    pub field: String,
    pub owner: String,
    pub detail: String,
    pub previous_value: String,
    pub reloaded_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Describes one extension-owned config drift detected during extension reload or status inspection.
pub struct ReloadOwnedExtensionDrift {
    pub field: String,
    pub owner: String,
    pub detail: String,
    pub previous_value: String,
    pub reloaded_value: String,
}

#[derive(Debug, Clone)]
/// Captures extension reload results plus restart-only runtime drift.
pub struct ExtensionReloadReport {
    pub extensions: ExtensionsReport,
    pub preserved_session: bool,
    pub session_before: Option<SessionSummary>,
    pub session_after: Option<SessionSummary>,
    pub restart_required_drifts: Vec<RestartRequiredRuntimeDrift>,
    pub reload_owned_drifts: Vec<ReloadOwnedExtensionDrift>,
    pub ownership_blocked: bool,
    pub ownership_baseline_path: std::path::PathBuf,
    pub ownership_baseline_source: String,
    pub ownership_baseline_snapshot: String,
}

#[derive(Debug, Clone)]
/// Surfaces the currently loaded runtime ownership baseline and whether the active config now requires a restart.
pub struct RuntimeOwnershipStatusReport {
    pub restart_required_drifts: Vec<RestartRequiredRuntimeDrift>,
    pub reload_owned_drifts: Vec<ReloadOwnedExtensionDrift>,
    pub ownership_blocked: bool,
    pub ownership_baseline_path: std::path::PathBuf,
    pub ownership_baseline_source: String,
    pub ownership_baseline_snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeConfigOwnershipBaseline {
    display_interface: Option<String>,
    hooks_auto_accept: Option<bool>,
    security_redact_secrets: Option<bool>,
    network_force_ipv4: Option<bool>,
    runtime_provider: Option<String>,
    runtime_model: Option<String>,
    runtime_ollama_base_url: Option<String>,
    runtime_llamacpp_base_url: Option<String>,
    runtime_embedded_model_path: Option<String>,
    extension_manifests_dir: Option<String>,
    extension_entries: Vec<vela_config::ResolvedExtensionConfigEntry>,
}

impl RuntimeConfigOwnershipBaseline {
    fn from_resolved_config(config: &ResolvedConfig) -> Self {
        Self {
            display_interface: config.display_interface.clone(),
            hooks_auto_accept: config.hooks_auto_accept,
            security_redact_secrets: config.security_redact_secrets,
            network_force_ipv4: config.network_force_ipv4,
            runtime_provider: config.runtime_provider.clone(),
            runtime_model: config.runtime_model.clone(),
            runtime_ollama_base_url: config.runtime_ollama_base_url.clone(),
            runtime_llamacpp_base_url: config.runtime_llamacpp_base_url.clone(),
            runtime_embedded_model_path: config.runtime_embedded_model_path.clone(),
            extension_manifests_dir: config.extension_manifests_dir.clone(),
            extension_entries: sorted_extension_entries(&config.extension_entries),
        }
    }

    fn into_resolved_config(self) -> ResolvedConfig {
        ResolvedConfig {
            display_interface: self.display_interface,
            hooks_auto_accept: self.hooks_auto_accept,
            security_redact_secrets: self.security_redact_secrets,
            network_force_ipv4: self.network_force_ipv4,
            runtime_provider: self.runtime_provider,
            runtime_model: self.runtime_model,
            runtime_ollama_base_url: self.runtime_ollama_base_url,
            runtime_llamacpp_base_url: self.runtime_llamacpp_base_url,
            runtime_embedded_model_path: self.runtime_embedded_model_path,
            extension_manifests_dir: self.extension_manifests_dir,
            extension_entries: self.extension_entries,
        }
    }

    fn summary_line(&self) -> String {
        [
            (
                "display.interface",
                serde_json::to_string(&self.display_interface)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "hooks.auto_accept",
                serde_json::to_string(&self.hooks_auto_accept)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "security.redact_secrets",
                serde_json::to_string(&self.security_redact_secrets)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "network.force_ipv4",
                serde_json::to_string(&self.network_force_ipv4)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "runtime.provider",
                serde_json::to_string(&self.runtime_provider)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "runtime.model",
                serde_json::to_string(&self.runtime_model)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "runtime.ollama_base_url",
                serde_json::to_string(&self.runtime_ollama_base_url)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "runtime.llamacpp_base_url",
                serde_json::to_string(&self.runtime_llamacpp_base_url)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "runtime.embedded_model_path",
                serde_json::to_string(&self.runtime_embedded_model_path)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "extensions.manifests_dir",
                serde_json::to_string(&self.extension_manifests_dir)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
            (
                "extensions.entries",
                serde_json::to_string(&self.extension_entries)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string()),
            ),
        ]
        .into_iter()
        .map(|(field, value)| format!("{field}={value}"))
        .collect::<Vec<_>>()
        .join(" ")
    }
}

fn sorted_extension_entries(
    entries: &[vela_config::ResolvedExtensionConfigEntry],
) -> Vec<vela_config::ResolvedExtensionConfigEntry> {
    let mut entries = entries.to_vec();
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    entries
}

impl RestartRequiredRuntimeDrift {
    /// Renders a bounded old/new diff for one restart-only runtime setting.
    pub fn owned_setting_diff(&self) -> String {
        format!(
            "previous={} reloaded={} action=restart-required",
            self.previous_value, self.reloaded_value
        )
    }
}

impl ReloadOwnedExtensionDrift {
    /// Renders a bounded old/new diff for one reload-owned extension setting.
    pub fn owned_setting_diff(&self, action: &str) -> String {
        format!(
            "previous={} reloaded={} action={}",
            self.previous_value, self.reloaded_value, action
        )
    }
}

impl ExtensionReloadReport {
    /// Renders a compact reload summary including restart-required config drift.
    pub fn summary_line(&self) -> String {
        let restart_required = if self.restart_required_drifts.is_empty() {
            "none".to_string()
        } else {
            self.restart_required_drifts
                .iter()
                .map(|item| format!("{}@{}", item.field, item.owner))
                .collect::<Vec<_>>()
                .join(",")
        };
        let reload_owned = if self.reload_owned_drifts.is_empty() {
            "none".to_string()
        } else {
            self.reload_owned_drifts
                .iter()
                .map(|item| format!("{}@{}", item.field, item.owner))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "{} session_preserved={} restart_required={} reload_owned={} ownership_blocked={}",
            self.extensions.summary_line(),
            self.preserved_session,
            restart_required,
            reload_owned,
            self.ownership_blocked,
        )
    }

    /// Renders the baseline checkpoint used for restart-only ownership enforcement.
    pub fn ownership_baseline_line(&self) -> String {
        format!(
            "path={} source={} values={}",
            self.ownership_baseline_path.display(),
            self.ownership_baseline_source,
            self.ownership_baseline_snapshot,
        )
    }

    pub fn ownership_block_reason(&self) -> Option<String> {
        self.ownership_blocked.then(|| {
            format!(
                "extension reload blocked by kernel-owned runtime drift: {} (restart vela with the updated config to refresh the ownership baseline at {})",
                self.restart_required_drifts
                    .iter()
                    .map(|item| format!("{}@{}", item.field, item.owner))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.ownership_baseline_path.display()
            )
        })
    }
}

impl RuntimeOwnershipStatusReport {
    /// Renders a compact status line for the current runtime ownership baseline and restart requirement.
    pub fn summary_line(&self) -> String {
        let restart_required = if self.restart_required_drifts.is_empty() {
            "none".to_string()
        } else {
            self.restart_required_drifts
                .iter()
                .map(|item| format!("{}@{}", item.field, item.owner))
                .collect::<Vec<_>>()
                .join(",")
        };
        let reload_owned = if self.reload_owned_drifts.is_empty() {
            "none".to_string()
        } else {
            self.reload_owned_drifts
                .iter()
                .map(|item| format!("{}@{}", item.field, item.owner))
                .collect::<Vec<_>>()
                .join(",")
        };
        let status = if self.ownership_blocked {
            "restart-required"
        } else if self.reload_owned_drifts.is_empty() {
            "aligned"
        } else {
            "reload-available"
        };
        format!(
            "path={} source={} status={} restart_required={} reload_owned={}",
            self.ownership_baseline_path.display(),
            self.ownership_baseline_source,
            status,
            restart_required,
            reload_owned,
        )
    }

    pub fn ownership_baseline_line(&self) -> String {
        format!(
            "path={} source={} values={}",
            self.ownership_baseline_path.display(),
            self.ownership_baseline_source,
            self.ownership_baseline_snapshot,
        )
    }
}

#[derive(Debug, Clone)]
/// Aggregates the initialized runtime subsystems and resolved configuration.
pub struct BootstrapReport {
    pub vela_home: std::path::PathBuf,
    pub active_profile: Option<String>,
    pub loaded_env_paths: Vec<std::path::PathBuf>,
    pub ignored_user_config: bool,
    pub config_sources: Vec<ConfigSource>,
    pub resolved_config: ResolvedConfig,
    pub persistence: PersistenceReport,
    pub memory: MemoryReport,
    pub skills: SkillsReport,
    pub reviews: ReviewReport,
    pub extensions: ExtensionsReport,
}

impl BootstrapReport {
    /// Renders a compact human-readable bootstrap summary.
    pub fn summary_line(&self) -> String {
        let profile = self
            .active_profile
            .as_deref()
            .map(|p| format!(" profile={p}"))
            .unwrap_or_default();
        let env_count = self.loaded_env_paths.len();
        let config_count = self
            .config_sources
            .iter()
            .filter(|source| {
                matches!(
                    source.kind,
                    vela_config::ConfigSourceKind::User
                        | vela_config::ConfigSourceKind::ProjectFallback
                )
            })
            .count();
        format!(
            "vela bootstrap ready: home={} env_files={} config_files={} ignore_user_config={} state_db_runs={} extensions_activated={} extensions_validated={} extensions_disabled={} extensions_failed={}{}",
            self.vela_home.display(),
            env_count,
            config_count,
            self.ignored_user_config,
            self.persistence.bootstrap_runs,
            self.extensions.activated_count,
            self.extensions.validated_count,
            self.extensions.disabled_count,
            self.extensions.failed_count,
            profile
        )
    }
}

/// Initializes config, persistence, memory, skills, review, and extension-registry subsystems.
pub fn initialize_bootstrap(
    active_profile: Option<String>,
    ignore_user_config: bool,
) -> Result<BootstrapReport> {
    let config = vela_config::initialize_config(active_profile, ignore_user_config)?;
    let persistence = vela_state::initialize_persistence(&config.vela_home)?;
    let memory = vela_memory::initialize_memory(&config.vela_home)?;
    let skills = vela_skills::initialize_skills(&config.vela_home)?;
    let reviews = vela_review::initialize_reviews(&config.vela_home)?;
    let extensions =
        vela_extensions::initialize_extensions(&config.vela_home, &config.resolved_config)?;
    ensure_runtime_config_ownership_baseline(&config.vela_home, &config.resolved_config)?;
    Ok(BootstrapReport::from_parts(
        config,
        persistence,
        memory,
        skills,
        reviews,
        extensions,
    ))
}

/// Emits a debug log once runtime bootstrap has completed.
pub fn bootstrap_banner() {
    tracing::debug!("vela-runtime bootstrap initialized");
}

/// Returns the current session id and title when one is active.
pub fn current_session_identity(bootstrap: &BootstrapReport) -> Result<Option<(String, String)>> {
    vela_state::current_session_identity(&bootstrap.persistence.state_db_path)
}

/// Returns the latest session summary when one exists.
pub fn current_session_summary(bootstrap: &BootstrapReport) -> Result<Option<SessionSummary>> {
    vela_state::current_session_summary(&bootstrap.persistence.state_db_path)
}

/// Returns the latest session summary for a command-scoped runtime session.
pub fn current_command_session_summary(
    bootstrap: &BootstrapReport,
    command_name: &str,
) -> Result<Option<SessionSummary>> {
    vela_state::current_command_session_summary(&bootstrap.persistence.state_db_path, command_name)
}

/// Reloads extension discovery from the latest extension config and manifest files without resetting kernel-owned runtime state.
pub fn reload_extensions(bootstrap: &BootstrapReport) -> Result<ExtensionReloadReport> {
    let session_before = current_session_summary(bootstrap)?;
    let (_, resolved_config) =
        vela_config::reload_config_snapshot(&bootstrap.vela_home, bootstrap.ignored_user_config)?;
    let extensions =
        vela_extensions::initialize_extensions(&bootstrap.vela_home, &resolved_config)?;
    let session_after = current_session_summary(bootstrap)?;
    let preserved_session = match (session_before.as_ref(), session_after.as_ref()) {
        (Some(before), Some(after)) => before.id == after.id,
        (None, None) => true,
        _ => false,
    };
    let ownership_status = inspect_runtime_ownership_status_for_config(
        &bootstrap.vela_home,
        &bootstrap.resolved_config,
        &resolved_config,
    )?;
    if !ownership_status.ownership_blocked {
        persist_runtime_config_ownership_baseline(&bootstrap.vela_home, &resolved_config)?;
    }
    Ok(ExtensionReloadReport {
        extensions,
        preserved_session,
        session_before,
        session_after,
        ownership_blocked: ownership_status.ownership_blocked,
        restart_required_drifts: ownership_status.restart_required_drifts,
        reload_owned_drifts: ownership_status.reload_owned_drifts,
        ownership_baseline_path: ownership_status.ownership_baseline_path,
        ownership_baseline_source: ownership_status.ownership_baseline_source,
        ownership_baseline_snapshot: ownership_status.ownership_baseline_snapshot,
    })
}

/// Surfaces the currently effective runtime ownership baseline and whether the active config requires a restart to reconcile drift.
pub fn inspect_runtime_ownership_status(
    bootstrap: &BootstrapReport,
) -> Result<RuntimeOwnershipStatusReport> {
    inspect_runtime_ownership_status_for_config(
        &bootstrap.vela_home,
        &bootstrap.resolved_config,
        &bootstrap.resolved_config,
    )
}

fn inspect_runtime_ownership_status_for_config(
    vela_home: &std::path::Path,
    fallback_config: &ResolvedConfig,
    current_config: &ResolvedConfig,
) -> Result<RuntimeOwnershipStatusReport> {
    let ownership_baseline_path = runtime_config_ownership_baseline_path(vela_home);
    let (previous_config, ownership_baseline_source) =
        match load_runtime_config_ownership_baseline(vela_home)? {
            Some(config) => (config, "durable-baseline".to_string()),
            None => (fallback_config.clone(), "bootstrap-fallback".to_string()),
        };
    let ownership_baseline_snapshot =
        RuntimeConfigOwnershipBaseline::from_resolved_config(&previous_config).summary_line();
    let restart_required_drifts = restart_required_runtime_drifts(&previous_config, current_config);
    let reload_owned_drifts = reload_owned_extension_drifts(&previous_config, current_config);
    let ownership_blocked = !restart_required_drifts.is_empty();
    Ok(RuntimeOwnershipStatusReport {
        restart_required_drifts,
        reload_owned_drifts,
        ownership_blocked,
        ownership_baseline_path,
        ownership_baseline_source,
        ownership_baseline_snapshot,
    })
}

pub(crate) fn runtime_config_ownership_baseline_path(
    vela_home: &std::path::Path,
) -> std::path::PathBuf {
    vela_home
        .join("runtime")
        .join("reload-ownership-baseline.json")
}

pub(crate) fn ensure_runtime_config_ownership_baseline(
    vela_home: &std::path::Path,
    resolved_config: &ResolvedConfig,
) -> Result<()> {
    let path = runtime_config_ownership_baseline_path(vela_home);
    if path.exists() {
        return Ok(());
    }
    persist_runtime_config_ownership_baseline(vela_home, resolved_config)
}

pub(crate) fn load_runtime_config_ownership_baseline(
    vela_home: &std::path::Path,
) -> Result<Option<ResolvedConfig>> {
    let path = runtime_config_ownership_baseline_path(vela_home);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(None);
    }
    let baseline: RuntimeConfigOwnershipBaseline = serde_json::from_str(&content)
        .with_context(|| format!("failed to decode {}", path.display()))?;
    Ok(Some(baseline.into_resolved_config()))
}

pub(crate) fn persist_runtime_config_ownership_baseline(
    vela_home: &std::path::Path,
    resolved_config: &ResolvedConfig,
) -> Result<()> {
    let path = runtime_config_ownership_baseline_path(vela_home);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("runtime ownership baseline path has no parent"))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let temp_path = parent.join(format!(
        "{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("reload-ownership-baseline.json"),
        unix_timestamp_nanos()
    ));
    let baseline = RuntimeConfigOwnershipBaseline::from_resolved_config(resolved_config);
    std::fs::write(&temp_path, serde_json::to_string_pretty(&baseline)?)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    std::fs::rename(&temp_path, &path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

fn render_runtime_config_value<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"<unserializable>\"".to_string())
}

fn restart_required_runtime_drifts(
    previous: &ResolvedConfig,
    reloaded: &ResolvedConfig,
) -> Vec<RestartRequiredRuntimeDrift> {
    macro_rules! drift {
        ($condition:expr, $field:expr, $owner:expr, $detail:expr, $previous:expr, $reloaded:expr) => {
            if $condition {
                Some(RestartRequiredRuntimeDrift {
                    field: $field.to_string(),
                    owner: $owner.to_string(),
                    detail: $detail.to_string(),
                    previous_value: render_runtime_config_value(&$previous),
                    reloaded_value: render_runtime_config_value(&$reloaded),
                })
            } else {
                None
            }
        };
    }

    [
        drift!(
            previous.display_interface != reloaded.display_interface,
            "display.interface",
            "kernel-interface",
            "display interface changes remain restart-only during extension reload",
            previous.display_interface,
            reloaded.display_interface
        ),
        drift!(
            previous.hooks_auto_accept != reloaded.hooks_auto_accept,
            "hooks.auto_accept",
            "kernel-hooks",
            "hook auto-accept policy remains restart-only during extension reload",
            previous.hooks_auto_accept,
            reloaded.hooks_auto_accept
        ),
        drift!(
            previous.security_redact_secrets != reloaded.security_redact_secrets,
            "security.redact_secrets",
            "kernel-security",
            "security redaction policy remains restart-only during extension reload",
            previous.security_redact_secrets,
            reloaded.security_redact_secrets
        ),
        drift!(
            previous.network_force_ipv4 != reloaded.network_force_ipv4,
            "network.force_ipv4",
            "kernel-network",
            "network stack settings remain restart-only during extension reload",
            previous.network_force_ipv4,
            reloaded.network_force_ipv4
        ),
        drift!(
            previous.runtime_provider != reloaded.runtime_provider,
            "runtime.provider",
            "kernel-runtime",
            "provider backend changes remain restart-only during extension reload",
            previous.runtime_provider,
            reloaded.runtime_provider
        ),
        drift!(
            previous.runtime_model != reloaded.runtime_model,
            "runtime.model",
            "kernel-runtime",
            "runtime model changes remain restart-only during extension reload",
            previous.runtime_model,
            reloaded.runtime_model
        ),
        drift!(
            previous.runtime_ollama_base_url != reloaded.runtime_ollama_base_url,
            "runtime.ollama_base_url",
            "kernel-runtime",
            "provider transport endpoint changes remain restart-only during extension reload",
            previous.runtime_ollama_base_url,
            reloaded.runtime_ollama_base_url
        ),
        drift!(
            previous.runtime_llamacpp_base_url != reloaded.runtime_llamacpp_base_url,
            "runtime.llamacpp_base_url",
            "kernel-runtime",
            "provider transport endpoint changes remain restart-only during extension reload",
            previous.runtime_llamacpp_base_url,
            reloaded.runtime_llamacpp_base_url
        ),
        drift!(
            previous.runtime_embedded_model_path != reloaded.runtime_embedded_model_path,
            "runtime.embedded_model_path",
            "kernel-runtime",
            "embedded model asset changes remain restart-only during extension reload",
            previous.runtime_embedded_model_path,
            reloaded.runtime_embedded_model_path
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn reload_owned_extension_drifts(
    previous: &ResolvedConfig,
    reloaded: &ResolvedConfig,
) -> Vec<ReloadOwnedExtensionDrift> {
    let mut drifts = Vec::new();
    if previous.extension_manifests_dir != reloaded.extension_manifests_dir {
        drifts.push(ReloadOwnedExtensionDrift {
            field: "extensions.manifests_dir".to_string(),
            owner: "extensions".to_string(),
            detail:
                "extension manifest directory changes reload immediately during extension reload"
                    .to_string(),
            previous_value: render_runtime_config_value(&previous.extension_manifests_dir),
            reloaded_value: render_runtime_config_value(&reloaded.extension_manifests_dir),
        });
    }

    let previous_entries: std::collections::BTreeMap<_, _> = previous
        .extension_entries
        .iter()
        .map(|entry| (entry.id.clone(), entry.enabled))
        .collect();
    let reloaded_entries: std::collections::BTreeMap<_, _> = reloaded
        .extension_entries
        .iter()
        .map(|entry| (entry.id.clone(), entry.enabled))
        .collect();
    let ids: std::collections::BTreeSet<_> = previous_entries
        .keys()
        .chain(reloaded_entries.keys())
        .cloned()
        .collect();
    for id in ids {
        let previous_enabled = previous_entries.get(&id).copied();
        let reloaded_enabled = reloaded_entries.get(&id).copied();
        if previous_enabled != reloaded_enabled {
            drifts.push(ReloadOwnedExtensionDrift {
                field: format!("extensions.entries.{id}.enabled"),
                owner: "extensions".to_string(),
                detail: extension_entry_drift_detail(previous_enabled, reloaded_enabled),
                previous_value: render_runtime_config_value(&previous_enabled),
                reloaded_value: render_runtime_config_value(&reloaded_enabled),
            });
        }
    }

    drifts
}

fn extension_entry_drift_detail(previous: Option<bool>, reloaded: Option<bool>) -> String {
    match (previous, reloaded) {
        (None, Some(_)) => {
            "extension enable/disable override additions reload immediately during extension reload"
                .to_string()
        }
        (Some(_), None) => {
            "extension enable/disable override removals reload immediately during extension reload"
                .to_string()
        }
        (Some(_), Some(_)) => {
            "extension enable/disable override value changes reload immediately during extension reload"
                .to_string()
        }
        (None, None) => "extension override drift unavailable".to_string(),
    }
}

/// Resolves or creates a runtime session for an interactive request.
pub fn resolve_runtime_session(
    bootstrap: &BootstrapReport,
    request: &SessionRequest,
) -> Result<SessionRuntimeReport> {
    vela_state::resolve_runtime_session(&bootstrap.persistence.state_db_path, request)
}

/// Ensures the durable gateway directory structure and config file exist.
pub fn setup_gateway(bootstrap: &BootstrapReport) -> Result<GatewaySetupReport> {
    let gateway_dir = bootstrap.vela_home.join("gateway");
    std::fs::create_dir_all(&gateway_dir)?;
    let inbox_dir = gateway_dir.join("inbox");
    let outbox_dir = gateway_dir.join("outbox");
    std::fs::create_dir_all(&inbox_dir)?;
    std::fs::create_dir_all(&outbox_dir)?;

    let config_path = gateway_dir.join("config.json");
    let config_existed_before = config_path.is_file();
    if !config_existed_before {
        std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "default_source": "gateway",
                "session_command_name": "gateway",
                "active_profile": bootstrap.active_profile,
                "display_interface": bootstrap.resolved_config.display_interface,
                "transport_mode": "local-bootstrap",
            }))?,
        )?;
    }

    Ok(GatewaySetupReport {
        gateway_dir,
        config_path,
        inbox_dir,
        outbox_dir,
        config_existed_before,
    })
}

/// Starts or resumes the durable gateway runtime session.
pub fn start_gateway(bootstrap: &BootstrapReport) -> Result<GatewayStartReport> {
    let setup = setup_gateway(bootstrap)?;
    let session = vela_state::resolve_command_session(
        &bootstrap.persistence.state_db_path,
        "gateway",
        InteractionMode::Interactive,
    )?;
    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
        "gateway_started",
        json!({
            "source": "gateway",
            "config_path": setup.config_path,
            "inbox_dir": setup.inbox_dir,
            "outbox_dir": setup.outbox_dir,
            "action": session.action.label(),
        })
        .to_string(),
    )?;
    if !event_logged {
        tracing::warn!(session_id=%session.session_id, "failed to append gateway_started event");
    }
    if matches!(session.action, SessionAction::Created) {
        let message_logged = vela_state::append_message_to_session(
            &bootstrap.persistence.state_db_path,
            &session.session_id,
            "system",
            "Gateway bootstrap ready.",
            Some(
                json!({
                    "source": "gateway",
                    "direction": "egress",
                    "config_path": setup.config_path,
                })
                .to_string(),
            ),
        )?;
        if !message_logged {
            tracing::warn!(session_id=%session.session_id, "failed to append gateway bootstrap message");
        }
    }
    Ok(GatewayStartReport { setup, session })
}

fn load_subagent_delegations(path: &std::path::Path) -> Result<Vec<SubagentDelegationRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&content)?)
}

fn save_subagent_delegations(
    path: &std::path::Path,
    records: &[SubagentDelegationRecord],
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("delegations path has no parent directory"))?;
    let temp_path = parent.join(format!(
        "{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("delegations.json"),
        unix_timestamp_nanos()
    ));
    std::fs::write(&temp_path, serde_json::to_string_pretty(records)?)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

fn acquire_subagent_delegations_lock(path: &std::path::Path) -> Result<SchedulerJobsLock> {
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
    bail!("timed out waiting for subagent delegations lock")
}

fn load_mcp_bridge_calls(path: &std::path::Path) -> Result<Vec<McpBridgeCallRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&content)?)
}

fn save_mcp_bridge_calls(path: &std::path::Path, records: &[McpBridgeCallRecord]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("mcp bridge path has no parent directory"))?;
    let temp_path = parent.join(format!(
        "{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("requests.json"),
        unix_timestamp_nanos()
    ));
    std::fs::write(&temp_path, serde_json::to_string_pretty(records)?)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

fn acquire_mcp_bridge_lock(path: &std::path::Path) -> Result<SchedulerJobsLock> {
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
    bail!("timed out waiting for mcp bridge lock")
}

fn default_model_lab_policy() -> ModelLabPolicyRecord {
    ModelLabPolicyRecord {
        version: 2,
        summary: "Deeper model-core work must stay bounded, reversible, and evidence-driven before it can influence the live kernel route.".to_string(),
        graduation_gates: vec![
            "document a bounded experiment surface before changing runtime routing".to_string(),
            "capture repeatable eval evidence across at least two backends".to_string(),
            "preserve restart-only ownership boundaries for runtime config and transport changes".to_string(),
        ],
        allowed_experiment_strategies: vec![
            "shadow-routing".to_string(),
            "offline replay".to_string(),
            "bounded backend comparison".to_string(),
        ],
        prohibited_behaviors: vec![
            "silent live-route mutation without an explicit bounded slot".to_string(),
            "remote model execution by default for local-backend slices".to_string(),
            "unreviewed persistence or policy mutation from experimental paths".to_string(),
        ],
        required_evidence: vec![
            "persisted eval runs with per-backend outcomes".to_string(),
            "bounded failure-path coverage".to_string(),
            "operator-visible docs or CLI inspection surface".to_string(),
        ],
        adapter_finetune_intake_criteria: vec![
            "candidate work must target an existing provider backend contract".to_string(),
            "eval evidence must compare at least two allowed backends or explain the single-backend constraint".to_string(),
            "provider capabilities and pass/fail outcomes must be visible before runtime influence".to_string(),
            "live runtime routing, config policy, and persistence defaults remain unchanged until a separate reviewed promotion slice".to_string(),
        ],
    }
}

fn backend_experiment_slot(
    id: &str,
    strategy: &str,
    summary: &str,
    hypothesis: &str,
    default_prompt: &str,
    allowed_backends: &[&str],
    unchanged_surfaces: &[&str],
    rollback_note: &str,
    promotion_criteria: &[&str],
) -> BackendExperimentSlotRecord {
    BackendExperimentSlotRecord {
        id: id.to_string(),
        status: "bounded-preview".to_string(),
        strategy: strategy.to_string(),
        summary: Some(summary.to_string()),
        hypothesis: Some(hypothesis.to_string()),
        default_prompt: default_prompt.to_string(),
        allowed_backends: allowed_backends
            .iter()
            .map(|backend| backend.to_string())
            .collect(),
        unchanged_surfaces: unchanged_surfaces
            .iter()
            .map(|surface| surface.to_string())
            .collect(),
        rollback_note: Some(rollback_note.to_string()),
        promotion_criteria: promotion_criteria
            .iter()
            .map(|criterion| criterion.to_string())
            .collect(),
    }
}

fn default_backend_experiment_slots() -> Vec<BackendExperimentSlotRecord> {
    vec![
        backend_experiment_slot(
            "ternary-preview",
            "shadow-routing",
            "Compare candidate backends under one durable experiment slot without altering the live kernel route.",
            "A future ternary or sparse router can be evaluated safely by replaying the same prompt across a bounded backend set before any runtime routing changes land.",
            "Evaluate whether a ternary-style routing candidate would preserve concise, grounded backend behavior for this request.",
            &["embedded", "mock", "llamacpp", "ollama"],
            &["runtime route", "runtime config", "persistent policy"],
            "Remove or ignore this bounded slot record; it does not alter runtime routing, config, or persisted policy.",
            &["no automatic runtime route mutation", "durable eval evidence remains inspectable"],
        ),
        backend_experiment_slot(
            "sparse-routing-preview",
            "shadow-routing",
            "Shadow a sparse-routing candidate through the durable eval harness so future routing experiments stay operator-visible without mutating the live route.",
            "A sparse-routing preview lane can reveal whether a narrower backend handoff still preserves concise grounded behavior before any routing policy changes land.",
            "Evaluate whether a sparse-routing candidate would keep this request concise, grounded, and reversible across the bounded backend set.",
            &["embedded", "mock", "llamacpp", "ollama"],
            &["runtime route", "runtime config", "persistent policy"],
            "Remove or ignore this bounded slot record; it does not alter runtime routing, config, or persisted policy.",
            &["no automatic runtime route mutation", "durable eval evidence remains inspectable"],
        ),
        backend_experiment_slot(
            "local-first-replay",
            "offline-replay",
            "Replay the same prompt through the published backends to compare local-first behavior without mutating the live route.",
            "A durable offline replay lane can reveal whether local-first backends stay concise and comparable before any operator changes the runtime default.",
            "Replay this request across the bounded backends and compare whether local-first execution stays concise and operator-visible.",
            &["embedded", "llamacpp", "mock", "ollama"],
            &["runtime route", "runtime config", "persistent policy"],
            "Remove or ignore this bounded slot record; it does not alter runtime routing, config, or persisted policy.",
            &["no automatic runtime route mutation", "durable eval evidence remains inspectable"],
        ),
        backend_experiment_slot(
            "adapter-intake-gate",
            "offline-replay",
            "Replay one bounded request across the published backends to judge whether a future adapter or fine-tune intake path would stay inside the current contract.",
            "An adapter-intake lane can keep future backend intake criteria durable and reversible by comparing the current contract surfaces before any new route is promoted.",
            "Replay this request across the bounded backends and note whether the current provider contract looks stable enough for a future adapter or fine-tune intake review.",
            &["embedded", "llamacpp", "mock", "ollama"],
            &["runtime route", "runtime config", "persistent policy"],
            "Remove or ignore this bounded slot record; it does not alter runtime routing, config, or persisted policy.",
            &["no automatic runtime route mutation", "durable eval evidence remains inspectable"],
        ),
        backend_experiment_slot(
            "capability-parity-scan",
            "bounded-backend-comparison",
            "Compare the published backend capability matrix under one bounded experiment slot so parity work stays explicit.",
            "A bounded parity scan can highlight backend capability differences early enough to guide future provider work without broadening the live kernel contract.",
            "Compare the bounded backend capability matrix for this request and highlight where behavior still differs across providers.",
            &["embedded", "mock", "llamacpp", "ollama"],
            &["runtime route", "runtime config", "persistent policy"],
            "Remove or ignore this bounded slot record; it does not alter runtime routing, config, or persisted policy.",
            &[
                "all compared backends must record pass/fail outcomes",
                "provider capability summaries must be visible for each backend",
                "promotion remains descriptive until a separate reviewed routing slice",
            ],
        ),
    ]
}

fn merged_backend_experiment_slots(
    existing: Vec<BackendExperimentSlotRecord>,
) -> Vec<BackendExperimentSlotRecord> {
    let defaults = default_backend_experiment_slots();
    let mut merged = existing;
    for slot in defaults {
        if let Some(existing_slot) = merged.iter_mut().find(|existing| existing.id == slot.id) {
            for backend in slot.allowed_backends {
                if !existing_slot
                    .allowed_backends
                    .iter()
                    .any(|item| item == &backend)
                {
                    existing_slot.allowed_backends.push(backend);
                }
            }
            if existing_slot.unchanged_surfaces.is_empty() {
                existing_slot.unchanged_surfaces = slot.unchanged_surfaces;
            }
            if existing_slot.rollback_note.is_none() {
                existing_slot.rollback_note = slot.rollback_note;
            }
            if existing_slot.promotion_criteria.is_empty() {
                existing_slot.promotion_criteria = slot.promotion_criteria;
            }
        } else {
            merged.push(slot);
        }
    }
    merged
}

fn load_backend_eval_runs(path: &std::path::Path) -> Result<Vec<BackendEvalRunRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&content)?)
}

fn load_backend_experiment_slots(
    path: &std::path::Path,
) -> Result<Vec<BackendExperimentSlotRecord>> {
    if !path.exists() {
        return Ok(default_backend_experiment_slots());
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(default_backend_experiment_slots());
    }
    let existing: Vec<BackendExperimentSlotRecord> = serde_json::from_str(&content)?;
    Ok(merged_backend_experiment_slots(existing))
}

fn ensure_backend_experiment_slots(
    path: &std::path::Path,
) -> Result<Vec<BackendExperimentSlotRecord>> {
    let slots = load_backend_experiment_slots(path)?;
    let content = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };
    let persisted_count = if content.trim().is_empty() {
        0
    } else {
        serde_json::from_str::<Vec<BackendExperimentSlotRecord>>(&content)
            .map(|items| items.len())
            .unwrap_or(0)
    };
    if !path.exists() || persisted_count != slots.len() {
        std::fs::write(path, serde_json::to_string_pretty(&slots)?)?;
    }
    Ok(slots)
}

fn merge_model_lab_policy_defaults(mut policy: ModelLabPolicyRecord) -> ModelLabPolicyRecord {
    let defaults = default_model_lab_policy();
    if policy.adapter_finetune_intake_criteria.is_empty() {
        policy.adapter_finetune_intake_criteria = defaults.adapter_finetune_intake_criteria;
    }
    policy
}

fn load_model_lab_policy(path: &std::path::Path) -> Result<ModelLabPolicyRecord> {
    if !path.exists() {
        return Ok(default_model_lab_policy());
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(default_model_lab_policy());
    }
    Ok(merge_model_lab_policy_defaults(serde_json::from_str(
        &content,
    )?))
}

fn model_lab_policy_needs_version_upgrade(path: &std::path::Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(true);
    }
    let value: serde_json::Value = serde_json::from_str(&content)?;
    let persisted_version = value
        .get("version")
        .and_then(|item| item.as_u64())
        .unwrap_or_default() as u32;
    let criteria_missing_or_empty = value
        .get("adapter_finetune_intake_criteria")
        .and_then(|item| item.as_array())
        .map_or(true, |items| items.is_empty());
    Ok(persisted_version < default_model_lab_policy().version && criteria_missing_or_empty)
}

fn ensure_model_lab_policy(path: &std::path::Path) -> Result<ModelLabPolicyRecord> {
    let needs_upgrade = model_lab_policy_needs_version_upgrade(path)?;
    let mut policy = load_model_lab_policy(path)?;
    if needs_upgrade {
        policy.version = default_model_lab_policy().version;
        std::fs::write(path, serde_json::to_string_pretty(&policy)?)?;
    }
    Ok(policy)
}

fn save_backend_eval_runs(path: &std::path::Path, runs: &[BackendEvalRunRecord]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("backend eval path has no parent directory"))?;
    let temp_path = parent.join(format!(
        "{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("runs.json"),
        unix_timestamp_nanos()
    ));
    std::fs::write(&temp_path, serde_json::to_string_pretty(runs)?)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

fn acquire_backend_eval_lock(path: &std::path::Path) -> Result<SchedulerJobsLock> {
    let lock_path = path.with_extension("json.lock");
    for _ in 0..100 {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_) => return Ok(SchedulerJobsLock { lock_path }),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => sleep(Duration::from_millis(25)),
            Err(err) => return Err(err.into()),
        }
    }
    bail!("timed out waiting for backend eval lock")
}

fn normalize_eval_backends(backends: &[String]) -> Vec<String> {
    backends
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| match item.to_ascii_lowercase().as_str() {
            "llama.cpp" => "llamacpp".to_string(),
            other => other.to_string(),
        })
        .collect()
}

fn resolve_backend_eval_backends(
    bootstrap: &BootstrapReport,
    backends: &[String],
    allowed_backends: Option<&[String]>,
    _context: Option<&str>,
) -> Result<Vec<String>> {
    let normalized_backends = normalize_eval_backends(backends);
    if !normalized_backends.is_empty() {
        return Ok(normalized_backends);
    }

    if let Some(contract) = resolve_runtime_backend_contract(&bootstrap.resolved_config, None)? {
        let configured_backend = contract.id.to_string();
        if let Some(allowed_backends) = allowed_backends {
            if allowed_backends
                .iter()
                .any(|backend| backend == &configured_backend)
            {
                return Ok(vec![configured_backend]);
            }
            return Ok(allowed_backends.to_vec());
        }
        return Ok(vec![configured_backend]);
    }

    if let Some(allowed_backends) = allowed_backends {
        return Ok(allowed_backends.to_vec());
    }

    bail!("backend eval requires at least one --backend <id> or runtime.provider in config")
}

/// Ensures the durable subagent delegation registry exists.
pub fn setup_subagent_delegations(
    bootstrap: &BootstrapReport,
) -> Result<SubagentDelegationSetupReport> {
    let agents_dir = bootstrap.vela_home.join("agents");
    std::fs::create_dir_all(&agents_dir)?;
    let delegations_path = agents_dir.join("delegations.json");
    let delegations_existed_before = delegations_path.is_file();
    if !delegations_existed_before {
        std::fs::write(&delegations_path, "[]\n")?;
    }
    let delegation_count = load_subagent_delegations(&delegations_path)?.len();
    Ok(SubagentDelegationSetupReport {
        agents_dir,
        delegations_path,
        delegations_existed_before,
        delegation_count,
    })
}

/// Records one bounded subagent delegation request through the kernel-owned runtime surface.
pub fn request_subagent_delegation(
    bootstrap: &BootstrapReport,
    role: &str,
    task: &str,
    note: Option<&str>,
) -> Result<SubagentDelegationRequestReport> {
    let role = role.trim();
    if role.is_empty() {
        bail!("delegation role cannot be empty");
    }
    let task = task.trim();
    if task.is_empty() {
        bail!("delegation task cannot be empty");
    }
    let note = note
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let setup = setup_subagent_delegations(bootstrap)?;
    let session = vela_state::resolve_command_session(
        &bootstrap.persistence.state_db_path,
        "agents",
        InteractionMode::Interactive,
    )?;
    let _lock = acquire_subagent_delegations_lock(&setup.delegations_path)?;
    let mut records = load_subagent_delegations(&setup.delegations_path)?;
    if records
        .iter()
        .any(|record| record.status == "pending" && record.role == role && record.task == task)
    {
        bail!(
            "delegation for role {:?} with task {:?} is already pending",
            role,
            task
        );
    }
    let now = unix_timestamp();
    let record = SubagentDelegationRecord {
        id: format!("delegation-{}", unix_timestamp_nanos()),
        role: role.to_string(),
        task: task.to_string(),
        status: "pending".to_string(),
        created_at: now,
        updated_at: now,
        session_id: session.session_id.clone(),
        note,
    };
    records.push(record.clone());
    save_subagent_delegations(&setup.delegations_path, &records)?;

    let event_logged = match vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
        "delegation_requested",
        json!({
            "delegation_id": record.id,
            "role": record.role,
            "task": record.task,
            "note": record.note,
            "delegations_path": setup.delegations_path,
            "source": "agents",
            "action": session.action.label(),
        })
        .to_string(),
    ) {
        Ok(logged) => logged,
        Err(err) => {
            tracing::warn!(session_id=%session.session_id, error=%err, "failed to append delegation_requested event");
            false
        }
    };
    if !event_logged {
        tracing::warn!(session_id=%session.session_id, "failed to append delegation_requested event");
    }
    let message_logged = match vela_state::append_message_to_session(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
        "system",
        &format!("Delegation requested for role {}.", record.role),
        Some(
            json!({
                "source": "agents",
                "direction": "egress",
                "delegation_id": record.id,
                "task": record.task,
                "note": record.note,
            })
            .to_string(),
        ),
    ) {
        Ok(logged) => logged,
        Err(err) => {
            tracing::warn!(session_id=%session.session_id, error=%err, "failed to append delegation request message");
            false
        }
    };
    if !message_logged {
        tracing::warn!(session_id=%session.session_id, "failed to append delegation request message");
    }

    Ok(SubagentDelegationRequestReport {
        setup,
        session,
        record,
    })
}

/// Returns every durable subagent delegation record currently stored for the repo.
pub fn list_subagent_delegations(
    bootstrap: &BootstrapReport,
) -> Result<Vec<SubagentDelegationRecord>> {
    let setup = setup_subagent_delegations(bootstrap)?;
    load_subagent_delegations(&setup.delegations_path)
}

/// Returns one durable subagent delegation record by id when it exists.
pub fn get_subagent_delegation(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<Option<SubagentDelegationRecord>> {
    let normalized = id.trim();
    if normalized.is_empty() {
        bail!("delegation id cannot be empty");
    }
    let records = list_subagent_delegations(bootstrap)?;
    Ok(records.into_iter().find(|record| record.id == normalized))
}

/// Ensures the durable MCP bridge registry exists.
pub fn setup_mcp_bridge(bootstrap: &BootstrapReport) -> Result<McpBridgeSetupReport> {
    let mcp_dir = bootstrap.vela_home.join("mcp");
    std::fs::create_dir_all(&mcp_dir)?;
    let requests_path = mcp_dir.join("requests.json");
    let requests_existed_before = requests_path.is_file();
    if !requests_existed_before {
        std::fs::write(&requests_path, "[]\n")?;
    }
    let request_count = load_mcp_bridge_calls(&requests_path)?.len();
    Ok(McpBridgeSetupReport {
        mcp_dir,
        requests_path,
        requests_existed_before,
        request_count,
    })
}

/// Records one bounded MCP bridge request through the kernel-owned runtime surface.
pub fn request_mcp_bridge_call(
    bootstrap: &BootstrapReport,
    server: &str,
    tool: &str,
    payload: &str,
    note: Option<&str>,
) -> Result<McpBridgeRequestReport> {
    let server = server.trim();
    if server.is_empty() {
        bail!("mcp bridge server cannot be empty");
    }
    let tool = tool.trim();
    if tool.is_empty() {
        bail!("mcp bridge tool cannot be empty");
    }
    let payload = payload.trim();
    if payload.is_empty() {
        bail!("mcp bridge payload cannot be empty");
    }
    serde_json::from_str::<serde_json::Value>(payload)
        .map_err(|err| anyhow::anyhow!("mcp bridge payload must be valid JSON: {err}"))?;
    let note = note
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let setup = setup_mcp_bridge(bootstrap)?;
    let session = vela_state::resolve_command_session(
        &bootstrap.persistence.state_db_path,
        "mcp",
        InteractionMode::Interactive,
    )?;
    let _lock = acquire_mcp_bridge_lock(&setup.requests_path)?;
    let mut records = load_mcp_bridge_calls(&setup.requests_path)?;
    if records.iter().any(|record| {
        record.status == "pending"
            && record.server == server
            && record.tool == tool
            && record.payload == payload
    }) {
        bail!(
            "mcp bridge request for server {:?}, tool {:?}, and payload {:?} is already pending",
            server,
            tool,
            payload
        );
    }
    let now = unix_timestamp();
    let record = McpBridgeCallRecord {
        id: format!("mcp-bridge-{}", unix_timestamp_nanos()),
        server: server.to_string(),
        tool: tool.to_string(),
        payload: payload.to_string(),
        status: "pending".to_string(),
        created_at: now,
        updated_at: now,
        session_id: session.session_id.clone(),
        note,
    };
    records.push(record.clone());
    save_mcp_bridge_calls(&setup.requests_path, &records)?;

    let event_logged = match vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
        "mcp_bridge_requested",
        json!({
            "request_id": record.id,
            "server": record.server,
            "tool": record.tool,
            "payload": record.payload,
            "note": record.note,
            "requests_path": setup.requests_path,
            "source": "mcp",
            "action": session.action.label(),
        })
        .to_string(),
    ) {
        Ok(logged) => logged,
        Err(err) => {
            tracing::warn!(session_id=%session.session_id, error=%err, "failed to append mcp_bridge_requested event");
            false
        }
    };
    if !event_logged {
        tracing::warn!(session_id=%session.session_id, "failed to append mcp_bridge_requested event");
    }
    let message_logged = match vela_state::append_message_to_session(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
        "system",
        &format!(
            "MCP bridge requested for server {} tool {}.",
            record.server, record.tool
        ),
        Some(
            json!({
                "source": "mcp",
                "direction": "egress",
                "request_id": record.id,
                "server": record.server,
                "tool": record.tool,
                "payload": record.payload,
                "note": record.note,
            })
            .to_string(),
        ),
    ) {
        Ok(logged) => logged,
        Err(err) => {
            tracing::warn!(session_id=%session.session_id, error=%err, "failed to append mcp bridge request message");
            false
        }
    };
    if !message_logged {
        tracing::warn!(session_id=%session.session_id, "failed to append mcp bridge request message");
    }

    Ok(McpBridgeRequestReport {
        setup,
        session,
        record,
    })
}

/// Returns every durable MCP bridge request currently stored for the repo.
pub fn list_mcp_bridge_calls(bootstrap: &BootstrapReport) -> Result<Vec<McpBridgeCallRecord>> {
    let setup = setup_mcp_bridge(bootstrap)?;
    load_mcp_bridge_calls(&setup.requests_path)
}

/// Returns one durable MCP bridge request by id when it exists.
pub fn get_mcp_bridge_call(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<Option<McpBridgeCallRecord>> {
    let normalized = id.trim();
    if normalized.is_empty() {
        bail!("mcp bridge request id cannot be empty");
    }
    let records = list_mcp_bridge_calls(bootstrap)?;
    Ok(records.into_iter().find(|record| record.id == normalized))
}

/// Ensures the durable backend eval registry exists.
pub fn setup_backend_evals(bootstrap: &BootstrapReport) -> Result<BackendEvalSetupReport> {
    let evals_dir = bootstrap.vela_home.join("evals");
    std::fs::create_dir_all(&evals_dir)?;
    let runs_path = evals_dir.join("runs.json");
    let slots_path = evals_dir.join("slots.json");
    let policy_path = evals_dir.join("policy.json");
    let runs_existed_before = runs_path.is_file();
    if !runs_existed_before {
        std::fs::write(&runs_path, "[]\n")?;
    }
    let slots_existed_before = slots_path.is_file();
    if !slots_existed_before {
        std::fs::write(
            &slots_path,
            serde_json::to_string_pretty(&default_backend_experiment_slots())?,
        )?;
    }
    let policy_existed_before = policy_path.is_file();
    if !policy_existed_before {
        std::fs::write(
            &policy_path,
            serde_json::to_string_pretty(&default_model_lab_policy())?,
        )?;
    }
    let _policy = ensure_model_lab_policy(&policy_path)?;
    let run_count = load_backend_eval_runs(&runs_path)?.len();
    let slot_count = ensure_backend_experiment_slots(&slots_path)?.len();
    Ok(BackendEvalSetupReport {
        evals_dir,
        runs_path,
        slots_path,
        policy_path,
        runs_existed_before,
        slots_existed_before,
        policy_existed_before,
        run_count,
        slot_count,
    })
}

/// Executes a repeatable bounded backend evaluation run and persists the results.
pub fn run_backend_eval(
    bootstrap: &BootstrapReport,
    prompt: &str,
    backends: &[String],
    model_override: Option<&str>,
) -> Result<BackendEvalRunReport> {
    run_backend_eval_internal(bootstrap, prompt, backends, model_override, None)
}

/// Executes one bounded architecture experiment slot against the selected or default backend set.
pub fn run_backend_eval_slot(
    bootstrap: &BootstrapReport,
    slot_id: &str,
    backends: &[String],
    model_override: Option<&str>,
) -> Result<BackendEvalRunReport> {
    let slot = get_backend_experiment_slot(bootstrap, slot_id)?
        .ok_or_else(|| anyhow::anyhow!("backend experiment slot {:?} not found", slot_id))?;
    let effective_backends = resolve_backend_eval_backends(
        bootstrap,
        backends,
        Some(&slot.allowed_backends),
        Some("backend experiment slot"),
    )?;
    run_backend_eval_internal(
        bootstrap,
        &slot.default_prompt,
        &effective_backends,
        model_override,
        Some(slot.id),
    )
}

fn summarize_backend_eval_results(
    run: &BackendEvalRunRecord,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let passed = run
        .results
        .iter()
        .filter(|item| item.status == "passed")
        .map(|item| item.backend_id.clone())
        .collect::<Vec<_>>();
    let failed = run
        .results
        .iter()
        .filter(|item| item.status != "passed")
        .map(|item| item.backend_id.clone())
        .collect::<Vec<_>>();

    let mut capability_groups = std::collections::BTreeMap::<String, Vec<String>>::new();
    for result in &run.results {
        capability_groups
            .entry(
                result
                    .provider_capabilities
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
            )
            .or_default()
            .push(result.backend_id.clone());
    }
    let capability_groups = capability_groups
        .into_iter()
        .map(|(caps, backends)| format!("{}=>{}", backends.join("+"), caps))
        .collect::<Vec<_>>();

    (passed, failed, capability_groups)
}

fn summarize_backend_eval_score(results: &[BackendEvalResultRecord]) -> Option<String> {
    if results.is_empty() {
        return None;
    }
    let passed = results
        .iter()
        .filter(|item| item.status == "passed")
        .count();
    let failed = results.len().saturating_sub(passed);
    let pass_rate_percent = (passed * 100) / results.len();
    Some(format!(
        "passed={} failed={} total={} pass_rate={}pct",
        passed,
        failed,
        results.len(),
        pass_rate_percent
    ))
}

fn summarize_backend_eval_parity(results: &[BackendEvalResultRecord]) -> Option<String> {
    if results.is_empty() {
        return None;
    }

    let synthetic_run = BackendEvalRunRecord {
        id: String::new(),
        prompt: String::new(),
        backends: Vec::new(),
        created_at: 0,
        session_id: String::new(),
        experiment_slot: None,
        model_override: None,
        parity_summary: None,
        score_summary: None,
        results: results.to_vec(),
    };
    let (passed, failed, capability_groups) = summarize_backend_eval_results(&synthetic_run);

    let capability_group_count = capability_groups.len();
    let parity = if results.len() < 2 {
        "single-backend"
    } else if failed.is_empty() && capability_group_count <= 1 {
        "aligned"
    } else {
        "diverged"
    };
    let capability_groups = capability_groups.join("; ");

    Some(format!(
        "parity={} passed={} failed={} capability_groups={} {}",
        parity,
        if passed.is_empty() {
            "none".to_string()
        } else {
            passed.join(",")
        },
        if failed.is_empty() {
            "none".to_string()
        } else {
            failed.join(",")
        },
        capability_group_count,
        capability_groups
    ))
}

fn summarize_latest_slot_backend_evidence(results: &[BackendEvalResultRecord]) -> Vec<String> {
    results
        .iter()
        .map(|result| {
            format!(
                "{}:{}@{} source={} model={}",
                result.backend_id,
                result.status,
                result.transport,
                result
                    .response_source
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
                result
                    .response_model
                    .clone()
                    .unwrap_or_else(|| "none".to_string())
            )
        })
        .collect()
}

fn run_backend_eval_internal(
    bootstrap: &BootstrapReport,
    prompt: &str,
    backends: &[String],
    model_override: Option<&str>,
    experiment_slot: Option<String>,
) -> Result<BackendEvalRunReport> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        bail!("backend eval prompt cannot be empty");
    }
    let normalized_backends = resolve_backend_eval_backends(bootstrap, backends, None, None)?;

    let setup = setup_backend_evals(bootstrap)?;
    let session = vela_state::resolve_command_session(
        &bootstrap.persistence.state_db_path,
        "eval",
        InteractionMode::Interactive,
    )?;

    let mut results = Vec::new();
    for backend in &normalized_backends {
        let started = std::time::Instant::now();
        let contract = resolve_runtime_backend_contract(&bootstrap.resolved_config, Some(backend))
            .ok()
            .flatten();
        let transport = contract
            .as_ref()
            .map(|item| item.transport.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let capabilities = contract
            .as_ref()
            .map(|item| item.capabilities.summary_line());
        let result = match resolve_runtime_execution(
            &bootstrap.resolved_config,
            Some(backend.as_str()),
            model_override,
        ) {
            Ok(execution) => {
                let duration_ms;
                match execution.provider.as_deref() {
                    Some(provider) => match provider
                        .validate()
                        .and_then(|_| provider.generate(prompt, None))
                    {
                        Ok(response) => {
                            duration_ms = started.elapsed().as_millis() as u64;
                            BackendEvalResultRecord {
                                backend_id: backend.clone(),
                                transport: transport.clone(),
                                status: "passed".to_string(),
                                duration_ms,
                                response_source: Some(
                                    provider.direct_response_source().to_string(),
                                ),
                                response_model: execution.model.clone(),
                                provider_capabilities: capabilities.clone(),
                                response_chars: response.chars().count(),
                                response_preview: Some(response.chars().take(120).collect()),
                                error: None,
                            }
                        }
                        Err(err) => {
                            duration_ms = started.elapsed().as_millis() as u64;
                            BackendEvalResultRecord {
                                backend_id: backend.clone(),
                                transport: transport.clone(),
                                status: "failed".to_string(),
                                duration_ms,
                                response_source: None,
                                response_model: execution.model.clone(),
                                provider_capabilities: capabilities.clone(),
                                response_chars: 0,
                                response_preview: None,
                                error: Some(err.to_string()),
                            }
                        }
                    },
                    None => {
                        duration_ms = started.elapsed().as_millis() as u64;
                        BackendEvalResultRecord {
                            backend_id: backend.clone(),
                            transport: transport.clone(),
                            status: "failed".to_string(),
                            duration_ms,
                            response_source: None,
                            response_model: execution.model.clone(),
                            provider_capabilities: capabilities.clone(),
                            response_chars: 0,
                            response_preview: None,
                            error: Some(
                                "backend did not resolve to a provider execution path".to_string(),
                            ),
                        }
                    }
                }
            }
            Err(err) => BackendEvalResultRecord {
                backend_id: backend.clone(),
                transport: transport.clone(),
                status: "failed".to_string(),
                duration_ms: started.elapsed().as_millis() as u64,
                response_source: None,
                response_model: model_override
                    .map(str::to_string)
                    .or_else(|| bootstrap.resolved_config.runtime_model.clone()),
                provider_capabilities: capabilities.clone(),
                response_chars: 0,
                response_preview: None,
                error: Some(err.to_string()),
            },
        };
        results.push(result);
    }

    let parity_summary = summarize_backend_eval_parity(&results);
    let score_summary = summarize_backend_eval_score(&results);
    let record = BackendEvalRunRecord {
        id: format!("eval-{}", unix_timestamp_nanos()),
        prompt: prompt.to_string(),
        backends: normalized_backends,
        created_at: unix_timestamp(),
        session_id: session.session_id.clone(),
        experiment_slot,
        model_override: model_override
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        parity_summary,
        score_summary,
        results,
    };

    let _lock = acquire_backend_eval_lock(&setup.runs_path)?;
    let mut runs = load_backend_eval_runs(&setup.runs_path)?;
    runs.push(record.clone());
    save_backend_eval_runs(&setup.runs_path, &runs)?;

    let event_logged = vela_state::append_event_to_session(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
        "backend_eval_completed",
        json!({
            "eval_id": record.id,
            "backends": record.backends,
            "experiment_slot": record.experiment_slot,
            "model_override": record.model_override,
            "results": record.results,
            "parity_summary": record.parity_summary,
            "score_summary": record.score_summary,
            "runs_path": setup.runs_path,
            "source": "eval",
            "action": session.action.label(),
        })
        .to_string(),
    )?;
    if !event_logged {
        tracing::warn!(session_id=%session.session_id, "failed to append backend_eval_completed event");
    }
    let message_logged = vela_state::append_message_to_session(
        &bootstrap.persistence.state_db_path,
        &session.session_id,
        "system",
        &format!(
            "Backend eval completed for {} backend(s).",
            record.backends.len()
        ),
        Some(
            json!({
                "source": "eval",
                "direction": "egress",
                "eval_id": record.id,
                "backends": record.backends,
                "experiment_slot": record.experiment_slot,
                "model_override": record.model_override,
                "parity_summary": record.parity_summary,
                "score_summary": record.score_summary,
            })
            .to_string(),
        ),
    )?;
    if !message_logged {
        tracing::warn!(session_id=%session.session_id, "failed to append backend eval message");
    }

    Ok(BackendEvalRunReport {
        setup,
        session,
        record,
    })
}

/// Returns every durable backend eval run currently stored for the repo.
pub fn list_backend_evals(bootstrap: &BootstrapReport) -> Result<Vec<BackendEvalRunRecord>> {
    let setup = setup_backend_evals(bootstrap)?;
    load_backend_eval_runs(&setup.runs_path)
}

/// Returns every bounded architecture experiment slot currently published for the repo.
pub fn list_backend_experiment_slots(
    bootstrap: &BootstrapReport,
) -> Result<Vec<BackendExperimentSlotRecord>> {
    let setup = setup_backend_evals(bootstrap)?;
    load_backend_experiment_slots(&setup.slots_path)
}

/// Returns every published experiment slot enriched with the latest durable eval evidence.
pub fn inspect_backend_experiment_slots(
    bootstrap: &BootstrapReport,
) -> Result<Vec<BackendExperimentSlotInspection>> {
    let slots = list_backend_experiment_slots(bootstrap)?;
    let runs = list_backend_evals(bootstrap)?;
    Ok(slots
        .into_iter()
        .map(|slot| {
            let latest = runs
                .iter()
                .rev()
                .find(|run| run.experiment_slot.as_deref() == Some(slot.id.as_str()));
            let (
                latest_eval_passed_backends,
                latest_eval_failed_backends,
                latest_eval_capability_groups,
            ) = latest
                .map(summarize_backend_eval_results)
                .unwrap_or_else(|| (Vec::new(), Vec::new(), Vec::new()));
            BackendExperimentSlotInspection {
                latest_eval_id: latest.map(|run| run.id.clone()),
                latest_eval_created_at: latest.map(|run| run.created_at),
                latest_eval_parity_summary: latest.and_then(|run| run.parity_summary.clone()),
                latest_eval_score_summary: latest.and_then(|run| run.score_summary.clone()),
                latest_eval_backends: latest.map(|run| run.backends.clone()).unwrap_or_default(),
                latest_eval_passed_backends,
                latest_eval_failed_backends,
                latest_eval_capability_groups,
                latest_eval_result_count: latest.map(|run| run.results.len()).unwrap_or(0),
                latest_backend_evidence: latest
                    .map(|run| summarize_latest_slot_backend_evidence(&run.results))
                    .unwrap_or_default(),
                slot,
            }
        })
        .collect())
}

/// Returns one published experiment slot enriched with the latest durable eval evidence.
pub fn get_backend_experiment_slot_inspection(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<Option<BackendExperimentSlotInspection>> {
    let normalized = id.trim();
    if normalized.is_empty() {
        bail!("backend experiment slot id cannot be empty");
    }
    let Some(slot) = get_backend_experiment_slot(bootstrap, normalized)? else {
        return Ok(None);
    };
    let runs = list_backend_evals(bootstrap)?;
    let latest = runs
        .iter()
        .rev()
        .find(|run| run.experiment_slot.as_deref() == Some(normalized));
    let (latest_eval_passed_backends, latest_eval_failed_backends, latest_eval_capability_groups) =
        latest
            .map(summarize_backend_eval_results)
            .unwrap_or_else(|| (Vec::new(), Vec::new(), Vec::new()));
    Ok(Some(BackendExperimentSlotInspection {
        latest_eval_id: latest.map(|run| run.id.clone()),
        latest_eval_created_at: latest.map(|run| run.created_at),
        latest_eval_parity_summary: latest.and_then(|run| run.parity_summary.clone()),
        latest_eval_score_summary: latest.and_then(|run| run.score_summary.clone()),
        latest_eval_backends: latest.map(|run| run.backends.clone()).unwrap_or_default(),
        latest_eval_passed_backends,
        latest_eval_failed_backends,
        latest_eval_capability_groups,
        latest_eval_result_count: latest.map(|run| run.results.len()).unwrap_or(0),
        latest_backend_evidence: latest
            .map(|run| summarize_latest_slot_backend_evidence(&run.results))
            .unwrap_or_default(),
        slot,
    }))
}

/// Returns the durable model-lab policy for the repo.
pub fn get_model_lab_policy(bootstrap: &BootstrapReport) -> Result<ModelLabPolicyRecord> {
    let setup = setup_backend_evals(bootstrap)?;
    load_model_lab_policy(&setup.policy_path)
}

/// Returns one bounded architecture experiment slot by id when it exists.
pub fn get_backend_experiment_slot(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<Option<BackendExperimentSlotRecord>> {
    let normalized = id.trim();
    if normalized.is_empty() {
        bail!("backend experiment slot id cannot be empty");
    }
    let slots = list_backend_experiment_slots(bootstrap)?;
    Ok(slots.into_iter().find(|slot| slot.id == normalized))
}

/// Returns one durable backend eval run by id when it exists.
pub fn get_backend_eval(
    bootstrap: &BootstrapReport,
    id: &str,
) -> Result<Option<BackendEvalRunRecord>> {
    let normalized = id.trim();
    if normalized.is_empty() {
        bail!("backend eval id cannot be empty");
    }
    let runs = list_backend_evals(bootstrap)?;
    Ok(runs.into_iter().find(|run| run.id == normalized))
}

/// Delivers a bounded outbound webhook payload through the durable gateway surface.
pub fn deliver_gateway_webhook(
    bootstrap: &BootstrapReport,
    url: &str,
    payload: &str,
    event_type: Option<&str>,
) -> Result<GatewayWebhookDeliveryReport> {
    let url = url.trim();
    if url.is_empty() {
        bail!("gateway webhook url cannot be empty");
    }
    let payload = payload.trim();
    if payload.is_empty() {
        bail!("gateway webhook payload cannot be empty");
    }
    let event_type = event_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gateway.webhook")
        .to_string();

    let start = start_gateway(bootstrap)?;
    let delivery_id = format!("gateway-webhook-{}", unix_timestamp_nanos());
    let outbox_record_path = start.setup.outbox_dir.join(format!("{delivery_id}.json"));
    let request_body = json!({
        "delivery_id": &delivery_id,
        "event_type": &event_type,
        "payload": payload,
        "session_id": &start.session.session_id,
        "source": "gateway",
        "active_profile": bootstrap.active_profile,
    });
    let request_text = serde_json::to_string(&request_body)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(request_text)
        .send();

    match response {
        Ok(response) => {
            let status_code = response.status().as_u16();
            let response_body = response.text().unwrap_or_default();
            if !(200..300).contains(&status_code) {
                std::fs::write(
                    &outbox_record_path,
                    serde_json::to_string_pretty(&json!({
                        "delivery_id": &delivery_id,
                        "result": "failed",
                        "url": url,
                        "event_type": &event_type,
                        "payload": payload,
                        "status_code": status_code,
                        "response_body": response_body,
                        "session_id": &start.session.session_id,
                        "source": "gateway",
                    }))?,
                )?;
                let event_logged = vela_state::append_event_to_session(
                    &bootstrap.persistence.state_db_path,
                    &start.session.session_id,
                    "gateway_webhook_delivery_failed",
                    json!({
                        "delivery_id": &delivery_id,
                        "url": url,
                        "event_type": &event_type,
                        "status_code": status_code,
                        "outbox_record_path": &outbox_record_path,
                    })
                    .to_string(),
                )?;
                if !event_logged {
                    tracing::warn!(session_id=%start.session.session_id, "failed to append gateway_webhook_delivery_failed event");
                }
                bail!("gateway webhook delivery failed with status {status_code}");
            }

            std::fs::write(
                &outbox_record_path,
                serde_json::to_string_pretty(&json!({
                    "delivery_id": &delivery_id,
                    "result": "delivered",
                    "url": url,
                    "event_type": &event_type,
                    "payload": payload,
                    "status_code": status_code,
                    "response_body": response_body,
                    "session_id": &start.session.session_id,
                    "source": "gateway",
                }))?,
            )?;
            let event_logged = vela_state::append_event_to_session(
                &bootstrap.persistence.state_db_path,
                &start.session.session_id,
                "gateway_webhook_delivered",
                json!({
                    "delivery_id": &delivery_id,
                    "url": url,
                    "event_type": &event_type,
                    "status_code": status_code,
                    "outbox_record_path": &outbox_record_path,
                    "source": "gateway",
                })
                .to_string(),
            )?;
            if !event_logged {
                tracing::warn!(session_id=%start.session.session_id, "failed to append gateway_webhook_delivered event");
            }
            let message_logged = vela_state::append_message_to_session(
                &bootstrap.persistence.state_db_path,
                &start.session.session_id,
                "system",
                &format!("Gateway webhook delivered to {url}."),
                Some(
                    json!({
                        "source": "gateway",
                        "direction": "egress",
                        "transport": "webhook",
                        "event_type": &event_type,
                        "outbox_record_path": &outbox_record_path,
                    })
                    .to_string(),
                ),
            )?;
            if !message_logged {
                tracing::warn!(session_id=%start.session.session_id, "failed to append gateway webhook message");
            }
            Ok(GatewayWebhookDeliveryReport {
                setup: start.setup,
                session: start.session,
                event_type,
                url: url.to_string(),
                outbox_record_path,
                status_code,
            })
        }
        Err(error) => {
            std::fs::write(
                &outbox_record_path,
                serde_json::to_string_pretty(&json!({
                    "delivery_id": &delivery_id,
                    "result": "failed",
                    "url": url,
                    "event_type": &event_type,
                    "payload": payload,
                    "error": error.to_string(),
                    "session_id": &start.session.session_id,
                    "source": "gateway",
                }))?,
            )?;
            let event_logged = vela_state::append_event_to_session(
                &bootstrap.persistence.state_db_path,
                &start.session.session_id,
                "gateway_webhook_delivery_failed",
                json!({
                    "delivery_id": &delivery_id,
                    "url": url,
                    "event_type": &event_type,
                    "error": error.to_string(),
                    "outbox_record_path": &outbox_record_path,
                    "source": "gateway",
                })
                .to_string(),
            )?;
            if !event_logged {
                tracing::warn!(session_id=%start.session.session_id, "failed to append gateway_webhook_delivery_failed event");
            }
            Err(error.into())
        }
    }
}
