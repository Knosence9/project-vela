use anyhow::{Context, Result};
use serde::Deserialize;
use serde_yaml::Value;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    pub vela_home: PathBuf,
    pub active_profile: Option<String>,
    pub loaded_env_paths: Vec<PathBuf>,
    pub ignored_user_config: bool,
    pub config_sources: Vec<ConfigSource>,
    pub resolved_config: ResolvedConfig,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedConfig {
    pub display_interface: Option<String>,
    pub hooks_auto_accept: Option<bool>,
    pub security_redact_secrets: Option<bool>,
    pub network_force_ipv4: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ConfigSource {
    pub path: PathBuf,
    pub kind: ConfigSourceKind,
}

#[derive(Debug, Clone, Copy)]
pub enum ConfigSourceKind {
    User,
    ProjectFallback,
    SkippedIgnored,
    SkippedLowerPrecedence,
    Missing,
}

impl ConfigSourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ProjectFallback => "project-fallback",
            Self::SkippedIgnored => "skipped-ignored",
            Self::SkippedLowerPrecedence => "skipped-lower-precedence",
            Self::Missing => "missing",
        }
    }
}

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
        if let Some((_, value)) = arg.split_once("--profile=") {
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

pub fn initialize_config(active_profile: Option<String>, ignore_user_config: bool) -> Result<BootstrapConfig> {
    let vela_home = compute_vela_home(active_profile.as_deref())?;
    env::set_var("VELA_HOME", &vela_home);
    std::fs::create_dir_all(&vela_home)
        .with_context(|| format!("failed to create {}", vela_home.display()))?;

    let effective_ignore_user_config = ignore_user_config || is_truthy_env("VELA_IGNORE_USER_CONFIG");
    if effective_ignore_user_config {
        env::set_var("VELA_IGNORE_USER_CONFIG", "1");
    }

    let loaded_env_paths = load_vela_dotenv(&vela_home)?;
    let config_sources = resolve_config_sources(&vela_home, effective_ignore_user_config)?;
    let resolved_config = load_resolved_config(&config_sources)?;

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

fn load_resolved_config(config_sources: &[ConfigSource]) -> Result<ResolvedConfig> {
    let mut merged = Value::Mapping(Default::default());

    for source in config_sources {
        if !matches!(source.kind, ConfigSourceKind::User | ConfigSourceKind::ProjectFallback) {
            continue;
        }
        let text = std::fs::read_to_string(&source.path)
            .with_context(|| format!("failed to read {}", source.path.display()))?;
        let parsed: Value = serde_yaml::from_str(&text)
            .with_context(|| format!("failed to parse {}", source.path.display()))?;
        merge_yaml(&mut merged, parsed);
    }

    let decoded: PartialConfig = serde_yaml::from_value(merged).unwrap_or_default();
    Ok(ResolvedConfig {
        display_interface: decoded.display.and_then(|d| d.interface),
        hooks_auto_accept: decoded.hooks_auto_accept,
        security_redact_secrets: decoded.security.and_then(|s| s.redact_secrets),
        network_force_ipv4: decoded.network.and_then(|n| n.force_ipv4),
    })
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
        });
    } else {
        sources.push(ConfigSource {
            path: user_config.clone(),
            kind: ConfigSourceKind::Missing,
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
