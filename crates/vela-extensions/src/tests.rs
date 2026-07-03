use super::*;

/// Builds a resolved config with extension-specific overrides for unit tests.
fn resolved_with(entries: Vec<ResolvedExtensionConfigEntry>) -> ResolvedConfig {
    ResolvedConfig {
        extension_manifests_dir: Some("extensions-manifests".to_string()),
        extension_entries: entries,
        ..ResolvedConfig::default()
    }
}

#[test]
/// Verifies that activation boundaries are explicit for supported, metadata-only, and disabled entries.
fn initialize_extensions_applies_activation_boundaries() {
    let vela_home = std::env::temp_dir().join(format!("vela-ext-test-{}", std::process::id()));
    let manifests_dir = vela_home.join("extensions-manifests");
    let _ = std::fs::remove_dir_all(&vela_home);
    std::fs::create_dir_all(&manifests_dir).unwrap();
    std::fs::write(
        manifests_dir.join("demo.yaml"),
        "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\nentry: extensions/demo-tool.wasm\ncapabilities:\n  - chat\n",
    )
    .unwrap();
    std::fs::write(
        manifests_dir.join("service.yaml"),
        "manifest_version: 1\nid: service\ntitle: Service\nkind: service\n",
    )
    .unwrap();
    std::fs::write(
        manifests_dir.join("ops.yaml"),
        "manifest_version: 1\nid: ops\ntitle: Ops\nkind: workflow\nentry: extensions/ops.flow\n",
    )
    .unwrap();
    std::fs::write(
        manifests_dir.join("notes.yaml"),
        "manifest_version: 1\nid: notes\ntitle: Notes\nkind: skill\nactivation: metadata-only\nentry: extensions/notes.md\n",
    )
    .unwrap();

    let report = initialize_extensions(
        &vela_home,
        &resolved_with(vec![ResolvedExtensionConfigEntry {
            id: "ops".to_string(),
            enabled: false,
        }]),
    )
    .unwrap();

    assert_eq!(report.discovered_manifest_count, 4);
    assert_eq!(report.discovered_count, 4);
    assert_eq!(report.activated_count, 1);
    assert_eq!(report.validated_count, 2);
    assert_eq!(report.disabled_count, 1);
    assert_eq!(report.failed_count, 0);
    assert!(report.entries.iter().any(|entry| {
        entry.id.as_deref() == Some("demo")
            && matches!(entry.lifecycle, ExtensionLifecycle::Activated)
            && entry.detail.as_deref() == Some("tool extension activated during bootstrap")
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.id.as_deref() == Some("service")
            && matches!(entry.lifecycle, ExtensionLifecycle::Validated)
            && entry.detail.as_deref()
                == Some("service extensions remain metadata-only in this slice")
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.id.as_deref() == Some("notes")
            && matches!(entry.lifecycle, ExtensionLifecycle::Validated)
            && entry.detail.as_deref()
                == Some("skill extension validated as metadata-only by manifest policy")
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.id.as_deref() == Some("ops")
            && matches!(entry.lifecycle, ExtensionLifecycle::Disabled)
    }));

    let _ = std::fs::remove_dir_all(&vela_home);
}

#[test]
/// Verifies that duplicate ids, unsupported manifests, and failed activation are surfaced as failed entries.
fn initialize_extensions_marks_failed_entries() {
    let vela_home = std::env::temp_dir().join(format!("vela-ext-invalid-{}", std::process::id()));
    let manifests_dir = vela_home.join("extensions-manifests");
    let _ = std::fs::remove_dir_all(&vela_home);
    std::fs::create_dir_all(&manifests_dir).unwrap();
    std::fs::write(
        manifests_dir.join("duplicate-a.yaml"),
        "manifest_version: 1\nid: duplicate\ntitle: First\nkind: tool\nentry: extensions/a.wasm\n",
    )
    .unwrap();
    std::fs::write(
        manifests_dir.join("duplicate-b.yaml"),
        "manifest_version: 1\nid: duplicate\ntitle: Second\nkind: tool\nentry: extensions/b.wasm\n",
    )
    .unwrap();
    std::fs::write(
        manifests_dir.join("unsupported.yaml"),
        "manifest_version: 2\nid: unsupported\ntitle: Unsupported\nkind: skill\n",
    )
    .unwrap();
    std::fs::write(
        manifests_dir.join("missing-entry.yaml"),
        "manifest_version: 1\nid: missing-entry\ntitle: Missing Entry\nkind: workflow\n",
    )
    .unwrap();
    std::fs::write(
        manifests_dir.join("service-on-boot.yaml"),
        "manifest_version: 1\nid: service-on-boot\ntitle: Service On Boot\nkind: service\nactivation: on-boot\n",
    )
    .unwrap();

    let report = initialize_extensions(&vela_home, &resolved_with(vec![])).unwrap();
    assert_eq!(report.discovered_manifest_count, 5);
    assert_eq!(report.discovered_count, 4);
    assert_eq!(report.activated_count, 0);
    assert_eq!(report.failed_count, 5);
    let duplicate_failed = report
        .entries
        .iter()
        .filter(|entry| {
            entry.id.as_deref() == Some("duplicate")
                && matches!(entry.lifecycle, ExtensionLifecycle::Failed)
                && entry.detail.as_deref() == Some("duplicate extension id discovered")
        })
        .count();
    assert_eq!(duplicate_failed, 2);
    assert!(report.entries.iter().any(|entry| {
        entry.id.as_deref() == Some("unsupported")
            && matches!(entry.lifecycle, ExtensionLifecycle::Failed)
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.id.as_deref() == Some("missing-entry")
            && matches!(entry.lifecycle, ExtensionLifecycle::Failed)
            && entry.detail.as_deref() == Some("activation requires a non-empty entry path")
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.id.as_deref() == Some("service-on-boot")
            && matches!(entry.lifecycle, ExtensionLifecycle::Failed)
            && entry.detail.as_deref()
                == Some("service extensions cannot request on-boot activation in this slice")
    }));

    let _ = std::fs::remove_dir_all(&vela_home);
}
