use super::*;

#[derive(Debug, Clone)]
/// Represents `BootstrapConfig` data exposed by this crate.
pub struct BootstrapConfig {
    pub vela_home: PathBuf,
    pub active_profile: Option<String>,
    pub loaded_env_paths: Vec<PathBuf>,
    pub ignored_user_config: bool,
    pub config_sources: Vec<ConfigSource>,
    pub resolved_config: ResolvedConfig,
}

#[derive(Debug, Clone, Default)]
/// Represents `ResolvedConfig` data exposed by this crate.
pub struct ResolvedConfig {
    pub display_interface: Option<String>,
    pub hooks_auto_accept: Option<bool>,
    pub security_redact_secrets: Option<bool>,
    pub network_force_ipv4: Option<bool>,
    pub runtime_provider: Option<String>,
    pub runtime_model: Option<String>,
    pub runtime_ollama_base_url: Option<String>,
    pub runtime_llamacpp_base_url: Option<String>,
    pub extension_manifests_dir: Option<String>,
    pub extension_entries: Vec<ResolvedExtensionConfigEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Describes one config-driven extension enable/disable override resolved from YAML.
pub struct ResolvedExtensionConfigEntry {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
/// Represents `ConfigSource` data exposed by this crate.
pub struct ConfigSource {
    pub path: PathBuf,
    pub kind: ConfigSourceKind,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy)]
/// Enumerates supported `ConfigSourceKind` variants.
pub enum ConfigSourceKind {
    User,
    ProjectFallback,
    SkippedIgnored,
    SkippedLowerPrecedence,
    SkippedUnreadable,
    SkippedInvalid,
    Missing,
}

impl ConfigSourceKind {
    /// Returns the stable string label used for persistence and display.
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ProjectFallback => "project-fallback",
            Self::SkippedIgnored => "skipped-ignored",
            Self::SkippedLowerPrecedence => "skipped-lower-precedence",
            Self::SkippedUnreadable => "skipped-unreadable",
            Self::SkippedInvalid => "skipped-invalid",
            Self::Missing => "missing",
        }
    }
}

/// Exposes the `preparse_profile_override` operation for this subsystem.
pub fn preparse_profile_override<I>(args: I) -> Result<(Vec<String>, Option<String>)>
where
    I: IntoIterator<Item = String>,
{
    let mut filtered = Vec::new();
    let mut iter = args.into_iter();
    let first = iter.next().context("missing argv[0]")?;
    filtered.push(first);

    let mut profile = None;
    while let Some(arg) = iter.next() {
        if arg == "--profile" || arg == "-p" {
            let value = iter.next().context("missing value for --profile/-p")?;
            profile = Some(value);
            continue;
        }
        if let Some(value) = arg.strip_prefix("--profile=") {
            profile = Some(value.to_string());
            continue;
        }
        filtered.push(arg);
    }

    let active = profile.or_else(read_sticky_profile);
    let vela_home = compute_vela_home(active.as_deref())?;
    env::set_var("VELA_HOME", &vela_home);

    Ok((filtered, active))
}

/// Initializes config state for this subsystem.
pub fn initialize_config(
    active_profile: Option<String>,
    ignore_user_config: bool,
) -> Result<BootstrapConfig> {
    let vela_home = compute_vela_home(active_profile.as_deref())?;
    env::set_var("VELA_HOME", &vela_home);
    std::fs::create_dir_all(&vela_home)
        .with_context(|| format!("failed to create {}", vela_home.display()))?;

    let loaded_env_paths = load_vela_dotenv(&vela_home)?;
    let (effective_ignore_user_config, config_sources, resolved_config) =
        load_config_snapshot(&vela_home, ignore_user_config)?;

    if effective_ignore_user_config {
        env::set_var("VELA_IGNORE_USER_CONFIG", "1");
    }
    if let Some(value) = resolved_config.hooks_auto_accept {
        env::set_var("VELA_ACCEPT_HOOKS", if value { "1" } else { "0" });
    }
    if let Some(value) = resolved_config.security_redact_secrets {
        env::set_var("VELA_REDACT_SECRETS", if value { "true" } else { "false" });
    }

    Ok(BootstrapConfig {
        vela_home,
        active_profile,
        loaded_env_paths,
        ignored_user_config: effective_ignore_user_config,
        config_sources,
        resolved_config,
    })
}

/// Reloads config sources and resolved settings for an already-selected VELA_HOME without reapplying env side effects.
pub fn reload_config_snapshot(
    vela_home: &Path,
    ignore_user_config: bool,
) -> Result<(Vec<ConfigSource>, ResolvedConfig)> {
    let (_, config_sources, resolved_config) = load_config_snapshot(vela_home, ignore_user_config)?;
    Ok((config_sources, resolved_config))
}
