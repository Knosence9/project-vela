use anyhow::{Context, Result};
use serde::Deserialize;
use serde_yaml::Value;
use std::env;
use std::path::{Path, PathBuf};

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
            let value = iter
                .next()
                .context("missing value for --profile/-p")?;
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
pub fn initialize_config(active_profile: Option<String>, ignore_user_config: bool) -> Result<BootstrapConfig> {
    let vela_home = compute_vela_home(active_profile.as_deref())?;
    env::set_var("VELA_HOME", &vela_home);
    std::fs::create_dir_all(&vela_home)
        .with_context(|| format!("failed to create {}", vela_home.display()))?;

    let loaded_env_paths = load_vela_dotenv(&vela_home)?;

    let effective_ignore_user_config = ignore_user_config || is_truthy_env("VELA_IGNORE_USER_CONFIG");
    if effective_ignore_user_config {
        env::set_var("VELA_IGNORE_USER_CONFIG", "1");
    }

    let mut config_sources = resolve_config_sources(&vela_home, effective_ignore_user_config)?;
    let resolved_config = load_resolved_config(&mut config_sources)?;

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

fn load_vela_dotenv(vela_home: &Path) -> Result<Vec<PathBuf>> {
    let mut loaded = Vec::new();
    let home_env = vela_home.join(".env");
    let project_env = env::current_dir()?.join(".env");

    if home_env.is_file() {
        dotenvy::from_path_override(&home_env)
            .with_context(|| format!("failed to load {}", home_env.display()))?;
        loaded.push(home_env);
    } else if project_env.is_file() {
        dotenvy::from_path_override(&project_env)
            .with_context(|| format!("failed to load {}", project_env.display()))?;
        loaded.push(project_env);
    }

    Ok(loaded)
}

fn load_resolved_config(config_sources: &mut [ConfigSource]) -> Result<ResolvedConfig> {
    let mut merged = Value::Mapping(Default::default());

    let mut user_loaded = false;
    if let Some(user_source) = config_sources
        .iter_mut()
        .find(|source| matches!(source.kind, ConfigSourceKind::User))
    {
        if let Some(parsed) = read_config_source(user_source) {
            merge_yaml(&mut merged, parsed);
            user_loaded = true;
        }
    }

    for source in config_sources.iter_mut() {
        if !matches!(
            source.kind,
            ConfigSourceKind::ProjectFallback | ConfigSourceKind::SkippedLowerPrecedence
        ) {
            continue;
        }
        if user_loaded {
            source.kind = ConfigSourceKind::SkippedLowerPrecedence;
            source.detail = None;
            continue;
        }
        source.kind = ConfigSourceKind::ProjectFallback;
        source.detail = None;
        if let Some(parsed) = read_config_source(source) {
            merge_yaml(&mut merged, parsed);
        }
    }

    let decoded: PartialConfig = serde_yaml::from_value(merged).unwrap_or_default();
    Ok(ResolvedConfig {
        display_interface: decoded.display.and_then(|d| d.interface),
        hooks_auto_accept: decoded.hooks_auto_accept,
        security_redact_secrets: decoded.security.and_then(|s| s.redact_secrets),
        network_force_ipv4: decoded.network.and_then(|n| n.force_ipv4),
        runtime_provider: decoded.runtime.as_ref().and_then(|r| r.provider.clone()),
        runtime_model: decoded.runtime.as_ref().and_then(|r| r.model.clone()),
        runtime_ollama_base_url: decoded.runtime.and_then(|r| r.ollama_base_url),
    })
}

fn read_config_source(source: &mut ConfigSource) -> Option<Value> {
    let text = match std::fs::read_to_string(&source.path) {
        Ok(text) => text,
        Err(error) => {
            source.kind = ConfigSourceKind::SkippedUnreadable;
            source.detail = Some(error.to_string());
            return None;
        }
    };
    let parsed: Value = match serde_yaml::from_str(&text) {
        Ok(parsed) => parsed,
        Err(error) => {
            source.kind = ConfigSourceKind::SkippedInvalid;
            source.detail = Some(error.to_string());
            return None;
        }
    };
    if let Err(error) = serde_yaml::from_value::<PartialConfig>(parsed.clone()) {
        source.kind = ConfigSourceKind::SkippedInvalid;
        source.detail = Some(error.to_string());
        return None;
    }
    source.detail = None;
    Some(parsed)
}

fn merge_yaml(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Mapping(base_map), Value::Mapping(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(&key) {
                    Some(existing) => merge_yaml(existing, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value;
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct PartialConfig {
    display: Option<DisplayConfig>,
    security: Option<SecurityConfig>,
    network: Option<NetworkConfig>,
    runtime: Option<RuntimeConfig>,
    hooks_auto_accept: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct DisplayConfig {
    interface: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SecurityConfig {
    redact_secrets: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct NetworkConfig {
    force_ipv4: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RuntimeConfig {
    provider: Option<String>,
    model: Option<String>,
    ollama_base_url: Option<String>,
}

fn resolve_config_sources(vela_home: &Path, ignore_user_config: bool) -> Result<Vec<ConfigSource>> {
    let mut sources = Vec::new();
    let user_config = vela_home.join("config.yaml");
    let project_config = env::current_dir()?.join("cli-config.yaml");

    let user_exists = user_config.is_file();
    let project_exists = project_config.is_file();

    if user_exists {
        sources.push(ConfigSource {
            path: user_config.clone(),
            kind: if ignore_user_config {
                ConfigSourceKind::SkippedIgnored
            } else {
                ConfigSourceKind::User
            },
            detail: None,
        });
    } else {
        sources.push(ConfigSource {
            path: user_config.clone(),
            kind: ConfigSourceKind::Missing,
            detail: None,
        });
    }

    let project_kind = if project_exists {
        if user_exists && !ignore_user_config {
            ConfigSourceKind::SkippedLowerPrecedence
        } else {
            ConfigSourceKind::ProjectFallback
        }
    } else {
        ConfigSourceKind::Missing
    };
    sources.push(ConfigSource {
        path: project_config,
        kind: project_kind,
        detail: None,
    });

    Ok(sources)
}

fn is_truthy_env(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => false,
    }
}

fn compute_vela_home(profile: Option<&str>) -> Result<PathBuf> {
    if let Some(explicit) = env::var_os("VELA_HOME") {
        return Ok(PathBuf::from(explicit));
    }

    let home = dirs::home_dir().context("home directory not available")?;
    let base = home.join(".vela");
    Ok(match profile {
        Some(profile) if !profile.trim().is_empty() => base.join("profiles").join(profile.trim()),
        _ => base,
    })
}

fn read_sticky_profile() -> Option<String> {
    let home = dirs::home_dir()?;
    let path = home.join(".vela").join("active_profile");
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_user_config_falls_back_to_project_config() {
        let root = std::env::temp_dir().join(format!("vela-config-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let user = root.join("user.yaml");
        let project = root.join("project.yaml");
        std::fs::write(&user, "display: [oops\n").unwrap();
        std::fs::write(&project, "display:\n  interface: tui\n").unwrap();

        let mut sources = vec![
            ConfigSource {
                path: user,
                kind: ConfigSourceKind::User,
                detail: None,
            },
            ConfigSource {
                path: project,
                kind: ConfigSourceKind::SkippedLowerPrecedence,
                detail: None,
            },
        ];

        let resolved = load_resolved_config(&mut sources).unwrap();
        assert_eq!(resolved.display_interface.as_deref(), Some("tui"));
        assert!(matches!(sources[0].kind, ConfigSourceKind::SkippedInvalid));
        assert!(sources[0].detail.is_some());
        assert!(matches!(sources[1].kind, ConfigSourceKind::ProjectFallback));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_user_config_falls_back_to_project_config() {
        let root = std::env::temp_dir().join(format!("vela-config-test-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let missing_user = root.join("missing-user.yaml");
        let project = root.join("project.yaml");
        std::fs::write(&project, "hooks_auto_accept: true\n").unwrap();

        let mut sources = vec![
            ConfigSource {
                path: missing_user,
                kind: ConfigSourceKind::User,
                detail: None,
            },
            ConfigSource {
                path: project,
                kind: ConfigSourceKind::SkippedLowerPrecedence,
                detail: None,
            },
        ];

        let resolved = load_resolved_config(&mut sources).unwrap();
        assert_eq!(resolved.hooks_auto_accept, Some(true));
        assert!(matches!(sources[0].kind, ConfigSourceKind::SkippedUnreadable));
        assert!(sources[0].detail.is_some());
        assert!(matches!(sources[1].kind, ConfigSourceKind::ProjectFallback));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn runtime_provider_settings_are_loaded_from_config() {
        let root = std::env::temp_dir().join(format!("vela-config-test-runtime-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let user = root.join("runtime.yaml");
        std::fs::write(
            &user,
            "runtime:\n  provider: ollama\n  model: gemma3:4b\n  ollama_base_url: http://127.0.0.1:11434\n",
        )
        .unwrap();

        let mut sources = vec![ConfigSource {
            path: user,
            kind: ConfigSourceKind::User,
            detail: None,
        }];

        let resolved = load_resolved_config(&mut sources).unwrap();
        assert_eq!(resolved.runtime_provider.as_deref(), Some("ollama"));
        assert_eq!(resolved.runtime_model.as_deref(), Some("gemma3:4b"));
        assert_eq!(
            resolved.runtime_ollama_base_url.as_deref(),
            Some("http://127.0.0.1:11434")
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
