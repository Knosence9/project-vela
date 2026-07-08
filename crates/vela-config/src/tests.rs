use super::*;
use std::ffi::OsString;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "vela-config-test-{label}-{}-{nonce}",
        std::process::id()
    ))
}

struct EnvGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }

    fn unset(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        std::env::remove_var(name);
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

struct CwdGuard {
    previous: std::path::PathBuf,
}

impl CwdGuard {
    fn change_to(path: &std::path::Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self { previous }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

#[test]
fn invalid_user_config_falls_back_to_project_config() {
    let root = unique_temp_dir("invalid-user");
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
    let root = unique_temp_dir("missing-user");
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
    assert!(matches!(
        sources[0].kind,
        ConfigSourceKind::SkippedUnreadable
    ));
    assert!(sources[0].detail.is_some());
    assert!(matches!(sources[1].kind, ConfigSourceKind::ProjectFallback));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ignore_user_config_env_promotes_project_fallback() {
    let _lock = test_lock().lock().unwrap();
    let home_root = unique_temp_dir("ignore-user-config-home");
    let project_root = unique_temp_dir("ignore-user-config-project");
    let vela_home = home_root.join(".vela");
    std::fs::create_dir_all(&vela_home).unwrap();
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(vela_home.join("config.yaml"), "display:\n  interface: tui\n").unwrap();
    std::fs::write(
        project_root.join("cli-config.yaml"),
        "display:\n  interface: text\n",
    )
    .unwrap();

    let _home = EnvGuard::set("HOME", &home_root);
    let _vela_home = EnvGuard::unset("VELA_HOME");
    let _ignore = EnvGuard::set("VELA_IGNORE_USER_CONFIG", "1");
    let _cwd = CwdGuard::change_to(&project_root);

    let bootstrap = initialize_config(None, false).unwrap();
    assert!(bootstrap.ignored_user_config);
    assert_eq!(bootstrap.resolved_config.display_interface.as_deref(), Some("text"));
    assert!(matches!(bootstrap.config_sources[0].kind, ConfigSourceKind::SkippedIgnored));
    assert!(matches!(bootstrap.config_sources[1].kind, ConfigSourceKind::ProjectFallback));

    let _ = std::fs::remove_dir_all(&home_root);
    let _ = std::fs::remove_dir_all(&project_root);
}

#[test]
fn initialize_config_prefers_vela_home_dotenv_over_project_fallback() {
    let _lock = test_lock().lock().unwrap();
    let home_root = unique_temp_dir("dotenv-home");
    let project_root = unique_temp_dir("dotenv-project");
    let vela_home = home_root.join(".vela");
    std::fs::create_dir_all(&vela_home).unwrap();
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(vela_home.join(".env"), "VELA_SESSION_SOURCE=home-env\n").unwrap();
    std::fs::write(project_root.join(".env"), "VELA_SESSION_SOURCE=project-env\n").unwrap();

    let _home = EnvGuard::set("HOME", &home_root);
    let _vela_home = EnvGuard::unset("VELA_HOME");
    let _session_source = EnvGuard::unset("VELA_SESSION_SOURCE");
    let _cwd = CwdGuard::change_to(&project_root);

    let bootstrap = initialize_config(None, false).unwrap();
    assert_eq!(bootstrap.loaded_env_paths, vec![vela_home.join(".env")]);
    assert_eq!(std::env::var("VELA_SESSION_SOURCE").ok().as_deref(), Some("home-env"));

    let _ = std::fs::remove_dir_all(&home_root);
    let _ = std::fs::remove_dir_all(&project_root);
}

#[test]
fn preparse_profile_override_uses_sticky_profile_and_sets_vela_home() {
    let _lock = test_lock().lock().unwrap();
    let home_root = unique_temp_dir("sticky-profile-home");
    let sticky_dir = home_root.join(".vela");
    std::fs::create_dir_all(&sticky_dir).unwrap();
    std::fs::write(sticky_dir.join("active_profile"), "work\n").unwrap();

    let _home = EnvGuard::set("HOME", &home_root);
    let _vela_home = EnvGuard::unset("VELA_HOME");

    let (filtered, active) = preparse_profile_override(vec!["vela".to_string()]).unwrap();
    assert_eq!(filtered, vec!["vela"]);
    assert_eq!(active.as_deref(), Some("work"));
    assert_eq!(
        std::env::var("VELA_HOME").ok().as_deref(),
        Some(home_root.join(".vela/profiles/work").to_string_lossy().as_ref())
    );

    let _ = std::fs::remove_dir_all(&home_root);
}

#[test]
fn runtime_provider_settings_are_loaded_from_config() {
    let root = unique_temp_dir("runtime");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let user = root.join("runtime.yaml");
    std::fs::write(
        &user,
        "runtime:\n  provider: llamacpp\n  model: gemma3:4b\n  ollama_base_url: http://127.0.0.1:11434\n  llamacpp_base_url: http://127.0.0.1:8080\n  embedded_model_path: /models/gemma3.gguf\n",
    )
    .unwrap();

    let mut sources = vec![ConfigSource {
        path: user,
        kind: ConfigSourceKind::User,
        detail: None,
    }];

    let resolved = load_resolved_config(&mut sources).unwrap();
    assert_eq!(resolved.runtime_provider.as_deref(), Some("llamacpp"));
    assert_eq!(resolved.runtime_model.as_deref(), Some("gemma3:4b"));
    assert_eq!(
        resolved.runtime_ollama_base_url.as_deref(),
        Some("http://127.0.0.1:11434")
    );
    assert_eq!(
        resolved.runtime_llamacpp_base_url.as_deref(),
        Some("http://127.0.0.1:8080")
    );
    assert_eq!(
        resolved.runtime_embedded_model_path.as_deref(),
        Some("/models/gemma3.gguf")
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn extension_settings_are_loaded_from_config() {
    let root = unique_temp_dir("extensions");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let user = root.join("extensions.yaml");
    std::fs::write(
        &user,
        "extensions:\n  manifests_dir: .vela/extensions/manifests\n  entries:\n    demo-tool:\n      enabled: false\n    demo-skill:\n      enabled: true\n",
    )
    .unwrap();

    let mut sources = vec![ConfigSource {
        path: user,
        kind: ConfigSourceKind::User,
        detail: None,
    }];

    let resolved = load_resolved_config(&mut sources).unwrap();
    assert_eq!(
        resolved.extension_manifests_dir.as_deref(),
        Some(".vela/extensions/manifests")
    );
    assert_eq!(
        resolved.extension_entries,
        vec![
            ResolvedExtensionConfigEntry {
                id: "demo-skill".to_string(),
                enabled: true,
            },
            ResolvedExtensionConfigEntry {
                id: "demo-tool".to_string(),
                enabled: false,
            },
        ]
    );

    let _ = std::fs::remove_dir_all(&root);
}
