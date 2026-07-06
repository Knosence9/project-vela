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
    pub last_session_id: Option<String>,
    #[serde(default)]
    pub execution_token: Option<String>,
    #[serde(default)]
    pub lease_expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Describes one kernel-owned runtime config drift detected during extension reload.
pub struct RestartRequiredRuntimeDrift {
    pub field: String,
    pub owner: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
/// Captures extension reload results plus restart-only runtime drift.
pub struct ExtensionReloadReport {
    pub extensions: ExtensionsReport,
    pub preserved_session: bool,
    pub session_before: Option<SessionSummary>,
    pub session_after: Option<SessionSummary>,
    pub restart_required_drifts: Vec<RestartRequiredRuntimeDrift>,
    pub ownership_blocked: bool,
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
        format!(
            "{} session_preserved={} restart_required={} ownership_blocked={}",
            self.extensions.summary_line(),
            self.preserved_session,
            restart_required,
            self.ownership_blocked,
        )
    }

    pub fn ownership_block_reason(&self) -> Option<String> {
        self.ownership_blocked.then(|| {
            format!(
                "extension reload blocked by kernel-owned runtime drift: {}",
                self.restart_required_drifts
                    .iter()
                    .map(|item| format!("{}@{}", item.field, item.owner))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
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
    let restart_required_drifts =
        restart_required_runtime_drifts(&bootstrap.resolved_config, &resolved_config);
    Ok(ExtensionReloadReport {
        extensions,
        preserved_session,
        session_before,
        session_after,
        ownership_blocked: !restart_required_drifts.is_empty(),
        restart_required_drifts,
    })
}

fn restart_required_runtime_drifts(
    previous: &ResolvedConfig,
    reloaded: &ResolvedConfig,
) -> Vec<RestartRequiredRuntimeDrift> {
    macro_rules! drift {
        ($condition:expr, $field:expr, $owner:expr, $detail:expr) => {
            if $condition {
                Some(RestartRequiredRuntimeDrift {
                    field: $field.to_string(),
                    owner: $owner.to_string(),
                    detail: $detail.to_string(),
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
            "display interface changes remain restart-only during extension reload"
        ),
        drift!(
            previous.hooks_auto_accept != reloaded.hooks_auto_accept,
            "hooks.auto_accept",
            "kernel-hooks",
            "hook auto-accept policy remains restart-only during extension reload"
        ),
        drift!(
            previous.security_redact_secrets != reloaded.security_redact_secrets,
            "security.redact_secrets",
            "kernel-security",
            "security redaction policy remains restart-only during extension reload"
        ),
        drift!(
            previous.network_force_ipv4 != reloaded.network_force_ipv4,
            "network.force_ipv4",
            "kernel-network",
            "network stack settings remain restart-only during extension reload"
        ),
        drift!(
            previous.runtime_provider != reloaded.runtime_provider,
            "runtime.provider",
            "kernel-runtime",
            "provider backend changes remain restart-only during extension reload"
        ),
        drift!(
            previous.runtime_model != reloaded.runtime_model,
            "runtime.model",
            "kernel-runtime",
            "runtime model changes remain restart-only during extension reload"
        ),
        drift!(
            previous.runtime_ollama_base_url != reloaded.runtime_ollama_base_url,
            "runtime.ollama_base_url",
            "kernel-runtime",
            "provider transport endpoint changes remain restart-only during extension reload"
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
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
