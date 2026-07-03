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
            "{} session_preserved={} restart_required={}",
            self.extensions.summary_line(),
            self.preserved_session,
            restart_required,
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
    Ok(ExtensionReloadReport {
        extensions,
        preserved_session,
        session_before,
        session_after,
        restart_required_drifts: restart_required_runtime_drifts(
            &bootstrap.resolved_config,
            &resolved_config,
        ),
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
