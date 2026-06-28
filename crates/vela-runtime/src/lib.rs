use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_yaml::Value;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BootstrapReport {
    pub vela_home: PathBuf,
    pub active_profile: Option<String>,
    pub loaded_env_paths: Vec<PathBuf>,
    pub ignored_user_config: bool,
    pub config_sources: Vec<ConfigSource>,
    pub resolved_config: ResolvedConfig,
    pub persistence: PersistenceReport,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedConfig {
    pub display_interface: Option<String>,
    pub hooks_auto_accept: Option<bool>,
    pub security_redact_secrets: Option<bool>,
    pub network_force_ipv4: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct PersistenceReport {
    pub state_db_path: PathBuf,
    pub sessions_dir: PathBuf,
    pub snapshot_pattern: String,
    pub state_db_existed_before: bool,
    pub bootstrap_runs: u64,
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
    SkippedUnreadable,
    SkippedInvalid,
    Missing,
}

impl ConfigSourceKind {
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

impl BootstrapReport {
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
            .filter(|source| matches!(source.kind, ConfigSourceKind::User | ConfigSourceKind::ProjectFallback))
            .count();
        format!(
            "vela bootstrap ready: home={} env_files={} config_files={} ignore_user_config={} state_db_runs={}{}",
            self.vela_home.display(),
            env_count,
            config_count,
            self.ignored_user_config,
            self.persistence.bootstrap_runs,
            profile
        )
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

pub fn initialize_bootstrap(active_profile: Option<String>, ignore_user_config: bool) -> Result<BootstrapReport> {
    let vela_home = compute_vela_home(active_profile.as_deref())?;
    env::set_var("VELA_HOME", &vela_home);
    std::fs::create_dir_all(&vela_home)
        .with_context(|| format!("failed to create {}", vela_home.display()))?;

    let effective_ignore_user_config = ignore_user_config || is_truthy_env("VELA_IGNORE_USER_CONFIG");
    if effective_ignore_user_config {
        env::set_var("VELA_IGNORE_USER_CONFIG", "1");
    }

    let loaded_env_paths = load_vela_dotenv(&vela_home)?;
    let mut config_sources = resolve_config_sources(&vela_home, effective_ignore_user_config)?;
    let resolved_config = load_resolved_config(&mut config_sources)?;
    let persistence = initialize_persistence(&vela_home)?;

    if let Some(value) = resolved_config.hooks_auto_accept {
        env::set_var("VELA_ACCEPT_HOOKS", if value { "1" } else { "0" });
    }
    if let Some(value) = resolved_config.security_redact_secrets {
        env::set_var("VELA_REDACT_SECRETS", if value { "true" } else { "false" });
    }

    Ok(BootstrapReport {
        vela_home,
        active_profile,
        loaded_env_paths,
        ignored_user_config: effective_ignore_user_config,
        config_sources,
        resolved_config,
        persistence,
    })
}

pub fn bootstrap_banner() {
    tracing::debug!("vela-runtime bootstrap initialized");
}

fn initialize_persistence(vela_home: &Path) -> Result<PersistenceReport> {
    let sessions_dir = vela_home.join("sessions");
    std::fs::create_dir_all(&sessions_dir)
        .with_context(|| format!("failed to create {}", sessions_dir.display()))?;

    let state_db_path = vela_home.join("state.db");
    let existed_before = state_db_path.is_file();
    let conn = Connection::open(&state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS state_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;

    let current_runs: Option<String> = conn
        .query_row(
            "SELECT value FROM state_meta WHERE key = 'bootstrap_runs'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let next_runs = current_runs
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
        + 1;

    conn.execute(
        "INSERT INTO state_meta(key, value) VALUES('bootstrap_runs', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![next_runs.to_string()],
    )?;

    conn.execute(
        "INSERT INTO state_meta(key, value) VALUES('snapshot_pattern', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params!["sessions/session_<id>.json"],
    )?;

    Ok(PersistenceReport {
        state_db_path,
        sessions_dir,
        snapshot_pattern: "sessions/session_<id>.json".to_string(),
        state_db_existed_before: existed_before,
        bootstrap_runs: next_runs,
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
            continue;
        }
        source.kind = ConfigSourceKind::ProjectFallback;
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
    })
}

fn read_config_source(source: &mut ConfigSource) -> Option<Value> {
    let text = match std::fs::read_to_string(&source.path) {
        Ok(text) => text,
        Err(_) => {
            source.kind = ConfigSourceKind::SkippedUnreadable;
            return None;
        }
    };
    match serde_yaml::from_str(&text) {
        Ok(parsed) => Some(parsed),
        Err(_) => {
            source.kind = ConfigSourceKind::SkippedInvalid;
            None
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_user_config_falls_back_to_project_config() {
        let root = std::env::temp_dir().join(format!("vela-runtime-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let user = root.join("user.yaml");
        let project = root.join("project.yaml");
        std::fs::write(&user, "display: [oops\n").unwrap();
        std::fs::write(&project, "display:\n  interface: tui\n").unwrap();

        let mut sources = vec![
            ConfigSource {
                path: user.clone(),
                kind: ConfigSourceKind::User,
            },
            ConfigSource {
                path: project.clone(),
                kind: ConfigSourceKind::SkippedLowerPrecedence,
            },
        ];

        let resolved = load_resolved_config(&mut sources).unwrap();
        assert_eq!(resolved.display_interface.as_deref(), Some("tui"));
        assert!(matches!(sources[0].kind, ConfigSourceKind::SkippedInvalid));
        assert!(matches!(sources[1].kind, ConfigSourceKind::ProjectFallback));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_user_config_falls_back_to_project_config() {
        let root = std::env::temp_dir().join(format!("vela-runtime-test-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let missing_user = root.join("missing-user.yaml");
        let project = root.join("project.yaml");
        std::fs::write(&project, "hooks_auto_accept: true\n").unwrap();

        let mut sources = vec![
            ConfigSource {
                path: missing_user,
                kind: ConfigSourceKind::User,
            },
            ConfigSource {
                path: project.clone(),
                kind: ConfigSourceKind::SkippedLowerPrecedence,
            },
        ];

        let resolved = load_resolved_config(&mut sources).unwrap();
        assert_eq!(resolved.hooks_auto_accept, Some(true));
        assert!(matches!(sources[0].kind, ConfigSourceKind::SkippedUnreadable));
        assert!(matches!(sources[1].kind, ConfigSourceKind::ProjectFallback));

        let _ = std::fs::remove_dir_all(&root);
    }
}
