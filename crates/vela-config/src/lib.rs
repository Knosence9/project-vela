use anyhow::{Context, Result};
use serde::Deserialize;
use serde_yaml::Value;
use std::env;
use std::path::{Path, PathBuf};

mod surface;

#[cfg(test)]
mod tests;

pub use surface::*;

fn load_config_snapshot(
    vela_home: &Path,
    ignore_user_config: bool,
) -> Result<(bool, Vec<ConfigSource>, ResolvedConfig)> {
    let effective_ignore_user_config =
        ignore_user_config || is_truthy_env("VELA_IGNORE_USER_CONFIG");
    let mut config_sources = resolve_config_sources(vela_home, effective_ignore_user_config)?;
    let resolved_config = load_resolved_config(&mut config_sources)?;
    Ok((
        effective_ignore_user_config,
        config_sources,
        resolved_config,
    ))
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
    let extension_entries = decoded
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.entries.as_ref())
        .map(|entries| {
            let mut collected: Vec<_> = entries
                .iter()
                .map(|(id, entry)| ResolvedExtensionConfigEntry {
                    id: id.trim().to_string(),
                    enabled: entry.enabled,
                })
                .filter(|entry| !entry.id.is_empty())
                .collect();
            collected.sort_by(|left, right| left.id.cmp(&right.id));
            collected
        })
        .unwrap_or_default();
    Ok(ResolvedConfig {
        display_interface: decoded.display.and_then(|d| d.interface),
        hooks_auto_accept: decoded.hooks_auto_accept,
        security_redact_secrets: decoded.security.and_then(|s| s.redact_secrets),
        network_force_ipv4: decoded.network.and_then(|n| n.force_ipv4),
        runtime_provider: decoded.runtime.as_ref().and_then(|r| r.provider.clone()),
        runtime_model: decoded.runtime.as_ref().and_then(|r| r.model.clone()),
        runtime_ollama_base_url: decoded
            .runtime
            .as_ref()
            .and_then(|r| r.ollama_base_url.clone()),
        runtime_llamacpp_base_url: decoded
            .runtime
            .as_ref()
            .and_then(|r| r.llamacpp_base_url.clone()),
        runtime_embedded_model_path: decoded.runtime.and_then(|r| r.embedded_model_path),
        extension_manifests_dir: decoded
            .extensions
            .and_then(|extensions| extensions.manifests_dir),
        extension_entries,
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
    extensions: Option<ExtensionsConfig>,
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
    llamacpp_base_url: Option<String>,
    embedded_model_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ExtensionsConfig {
    manifests_dir: Option<String>,
    entries: Option<std::collections::BTreeMap<String, ExtensionEntryConfig>>,
}

#[derive(Debug, Deserialize)]
struct ExtensionEntryConfig {
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
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
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
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
