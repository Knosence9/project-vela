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
    let root =
        std::env::temp_dir().join(format!("vela-config-test-missing-{}", std::process::id()));
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
fn runtime_provider_settings_are_loaded_from_config() {
    let root =
        std::env::temp_dir().join(format!("vela-config-test-runtime-{}", std::process::id()));
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

#[test]
fn extension_settings_are_loaded_from_config() {
    let root = std::env::temp_dir().join(format!(
        "vela-config-test-extensions-{}",
        std::process::id()
    ));
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
