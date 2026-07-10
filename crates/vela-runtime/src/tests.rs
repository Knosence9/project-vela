use super::*;

/// Creates an isolated bootstrap report for runtime regression tests.
fn test_bootstrap(prefix: &str) -> BootstrapReport {
    let vela_home =
        std::env::temp_dir().join(format!("vela-runtime-{prefix}-{}", unix_timestamp_nanos()));
    BootstrapReport {
        vela_home: vela_home.clone(),
        active_profile: None,
        loaded_env_paths: vec![],
        ignored_user_config: false,
        config_sources: vec![],
        resolved_config: ResolvedConfig::default(),
        persistence: vela_state::initialize_persistence(&vela_home).unwrap(),
        memory: vela_memory::initialize_memory(&vela_home).unwrap(),
        skills: vela_skills::initialize_skills(&vela_home).unwrap(),
        reviews: vela_review::initialize_reviews(&vela_home).unwrap(),
        extensions: vela_extensions::initialize_extensions(&vela_home, &ResolvedConfig::default())
            .unwrap(),
    }
}

#[test]
/// Verifies that the supported backend API contracts stay explicit and stable for current bounded backends.
fn supported_runtime_backend_contracts_are_explicit() {
    let contracts = supported_runtime_backend_contracts();
    assert_eq!(contracts.len(), 4);
    assert!(contracts.iter().any(|contract| {
        contract.id == "ollama"
            && contract.transport == "http-json"
            && contract.requires_model
            && contract.default_base_url == Some("http://127.0.0.1:11434")
            && contract.direct_response_source == "runtime-ollama"
            && contract.tool_loop_response_source == "runtime-ollama-tool-loop"
            && contract.capabilities.supports_images
    }));
    assert!(contracts.iter().any(|contract| {
        contract.id == "mock"
            && contract.transport == "in-process"
            && !contract.requires_model
            && contract.default_base_url.is_none()
            && contract.direct_response_source == "runtime-mock"
            && contract.tool_loop_response_source == "runtime-mock-tool-loop"
            && contract.capabilities.supports_tool_loop
    }));
    assert!(contracts.iter().any(|contract| {
        contract.id == "llamacpp"
            && contract.transport == "http-json"
            && contract.requires_model
            && contract.default_base_url == Some("http://127.0.0.1:8080")
            && contract.direct_response_source == "runtime-llamacpp"
            && contract.tool_loop_response_source == "runtime-llamacpp-tool-loop"
            && !contract.capabilities.supports_images
    }));
    assert!(contracts.iter().any(|contract| {
        contract.id == "embedded"
            && contract.transport == "in-process"
            && !contract.requires_model
            && contract.default_base_url.is_none()
            && contract.direct_response_source == "runtime-embedded"
            && contract.tool_loop_response_source == "runtime-embedded-tool-loop"
            && contract.capabilities.supports_text
            && contract.capabilities.supports_tool_loop
            && contract.capabilities.supports_reflection_retry
            && !contract.capabilities.supports_images
    }));
}

#[test]
/// Verifies that runtime execution resolves the Ollama backend through the provider boundary.
fn resolve_runtime_execution_wraps_ollama_provider_backend() {
    let resolved = ResolvedConfig {
        runtime_provider: Some("ollama".to_string()),
        runtime_model: Some("gemma3:4b".to_string()),
        runtime_ollama_base_url: Some("http://127.0.0.1:11434".to_string()),
        ..ResolvedConfig::default()
    };

    let execution = resolve_runtime_execution(&resolved, None, None).unwrap();
    let provider = execution
        .provider
        .as_deref()
        .expect("resolved provider backend");

    assert_eq!(execution.provider_label.as_deref(), Some("ollama"));
    assert_eq!(
        execution.provider_capabilities,
        Some(RuntimeProviderCapabilities {
            supports_text: true,
            supports_tool_loop: true,
            supports_reflection_retry: true,
            supports_images: true,
        })
    );
    assert_eq!(execution.model.as_deref(), Some("gemma3:4b"));
    assert_eq!(provider.label(), "ollama");
    assert_eq!(provider.model(), Some("gemma3:4b"));
    assert_eq!(provider.direct_response_source(), "runtime-ollama");
    assert_eq!(
        provider.tool_loop_response_source(),
        "runtime-ollama-tool-loop"
    );
    provider.validate().unwrap();
}

#[test]
/// Verifies that backend API resolution stays aligned with config and override semantics.
fn resolve_runtime_backend_contract_prefers_override_and_config() {
    let resolved = ResolvedConfig {
        runtime_provider: Some("ollama".to_string()),
        runtime_model: Some("gemma3:4b".to_string()),
        runtime_ollama_base_url: Some("http://127.0.0.1:11434".to_string()),
        ..ResolvedConfig::default()
    };

    let configured = resolve_runtime_backend_contract(&resolved, None)
        .unwrap()
        .expect("configured backend contract");
    assert_eq!(configured.id, "ollama");
    assert_eq!(configured.transport, "http-json");

    let overridden = resolve_runtime_backend_contract(&resolved, Some("mock"))
        .unwrap()
        .expect("override backend contract");
    assert_eq!(overridden.id, "mock");
    assert_eq!(overridden.transport, "in-process");

    let llamacpp = resolve_runtime_backend_contract(&resolved, Some("llama.cpp"))
        .unwrap()
        .expect("llama.cpp backend contract");
    assert_eq!(llamacpp.id, "llamacpp");
    assert_eq!(llamacpp.transport, "http-json");

    let embedded = resolve_runtime_backend_contract(&resolved, Some("embedded"))
        .unwrap()
        .expect("embedded backend contract");
    assert_eq!(embedded.id, "embedded");
    assert_eq!(embedded.transport, "in-process");

    let err = resolve_runtime_backend_contract(&resolved, Some("unknown")).unwrap_err();
    assert!(err.to_string().contains("unsupported runtime provider"));
}

#[test]
/// Verifies that runtime execution resolves the llama.cpp backend through the provider boundary.
fn resolve_runtime_execution_wraps_llamacpp_provider_backend() {
    let resolved = ResolvedConfig {
        runtime_provider: Some("llamacpp".to_string()),
        runtime_model: Some("phi-3-mini".to_string()),
        runtime_llamacpp_base_url: Some("http://127.0.0.1:8080".to_string()),
        ..ResolvedConfig::default()
    };

    let execution = resolve_runtime_execution(&resolved, None, None).unwrap();
    let provider = execution
        .provider
        .as_deref()
        .expect("resolved provider backend");

    assert_eq!(execution.provider_label.as_deref(), Some("llamacpp"));
    assert_eq!(
        execution.provider_capabilities,
        Some(RuntimeProviderCapabilities {
            supports_text: true,
            supports_tool_loop: true,
            supports_reflection_retry: true,
            supports_images: false,
        })
    );
    assert_eq!(execution.model.as_deref(), Some("phi-3-mini"));
    assert_eq!(provider.label(), "llamacpp");
    assert_eq!(provider.model(), Some("phi-3-mini"));
    assert_eq!(provider.direct_response_source(), "runtime-llamacpp");
    assert_eq!(
        provider.tool_loop_response_source(),
        "runtime-llamacpp-tool-loop"
    );
    assert!(!provider.supports_images());
    provider.validate().unwrap();
}

#[test]
/// Verifies that runtime execution resolves the embedded backend through the provider boundary and validates its model asset path.
fn resolve_runtime_execution_wraps_embedded_provider_backend() {
    let root =
        std::env::temp_dir().join(format!("vela-runtime-embedded-{}", unix_timestamp_nanos()));
    std::fs::create_dir_all(&root).unwrap();
    let model_path = root.join("gemma3.gguf");
    std::fs::write(&model_path, b"stub model").unwrap();

    let resolved = ResolvedConfig {
        runtime_provider: Some("embedded".to_string()),
        runtime_embedded_model_path: Some(model_path.display().to_string()),
        ..ResolvedConfig::default()
    };

    let execution = resolve_runtime_execution(&resolved, None, None).unwrap();
    let provider = execution
        .provider
        .as_deref()
        .expect("resolved embedded provider backend");

    assert_eq!(execution.provider_label.as_deref(), Some("embedded"));
    assert_eq!(provider.label(), "embedded");
    assert_eq!(provider.direct_response_source(), "runtime-embedded");
    assert_eq!(
        provider.tool_loop_response_source(),
        "runtime-embedded-tool-loop"
    );
    assert_eq!(
        execution.provider_capabilities,
        Some(RuntimeProviderCapabilities {
            supports_text: true,
            supports_tool_loop: true,
            supports_reflection_retry: true,
            supports_images: false,
        })
    );
    provider.validate().unwrap();

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
/// Verifies that embedded provider configuration fails clearly when the model path is missing.
fn embedded_provider_rejects_missing_model_path() {
    let resolved = ResolvedConfig {
        runtime_provider: Some("embedded".to_string()),
        ..ResolvedConfig::default()
    };

    let execution = resolve_runtime_execution(&resolved, None, None).unwrap();
    let provider = execution
        .provider
        .as_deref()
        .expect("resolved embedded provider backend");
    let err = provider.validate().unwrap_err();
    assert!(err
        .to_string()
        .contains("runtime provider 'embedded' requires runtime.embedded_model_path"));
}

#[test]
/// Verifies that embedded provider configuration rejects non-GGUF model paths before execution.
fn embedded_provider_rejects_non_gguf_model_path() {
    let root = std::env::temp_dir().join(format!(
        "vela-runtime-embedded-invalid-ext-{}",
        unix_timestamp_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let model_path = root.join("gemma3.txt");
    std::fs::write(&model_path, b"not a gguf").unwrap();

    let resolved = ResolvedConfig {
        runtime_provider: Some("embedded".to_string()),
        runtime_embedded_model_path: Some(model_path.display().to_string()),
        ..ResolvedConfig::default()
    };

    let execution = resolve_runtime_execution(&resolved, None, None).unwrap();
    let provider = execution
        .provider
        .as_deref()
        .expect("resolved embedded provider backend");
    let err = provider.validate().unwrap_err();
    assert!(err
        .to_string()
        .contains("runtime provider 'embedded' requires runtime.embedded_model_path to point to a .gguf model file"));

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
/// Verifies that the llama.cpp provider keeps local-only endpoint policy by default.
fn llamacpp_provider_rejects_remote_base_url_without_opt_in() {
    let resolved = ResolvedConfig {
        runtime_provider: Some("llamacpp".to_string()),
        runtime_model: Some("phi-3-mini".to_string()),
        runtime_llamacpp_base_url: Some("http://10.0.0.15:8080".to_string()),
        ..ResolvedConfig::default()
    };

    let execution = resolve_runtime_execution(&resolved, None, None).unwrap();
    let provider = execution
        .provider
        .as_deref()
        .expect("resolved provider backend");
    let err = provider.validate().unwrap_err();
    assert!(err
        .to_string()
        .contains("refusing non-local llama.cpp endpoint"));
}

#[test]
/// Verifies that runtime execution resolves the mock backend through the provider boundary.
fn resolve_runtime_execution_wraps_mock_provider_backend() {
    let resolved = ResolvedConfig {
        runtime_provider: Some("mock".to_string()),
        runtime_model: Some("mock-1".to_string()),
        ..ResolvedConfig::default()
    };

    let execution = resolve_runtime_execution(&resolved, None, None).unwrap();
    let provider = execution
        .provider
        .as_deref()
        .expect("resolved provider backend");

    assert_eq!(execution.provider_label.as_deref(), Some("mock"));
    assert_eq!(
        execution.provider_capabilities,
        Some(RuntimeProviderCapabilities {
            supports_text: true,
            supports_tool_loop: true,
            supports_reflection_retry: true,
            supports_images: true,
        })
    );
    assert_eq!(execution.model.as_deref(), Some("mock-1"));
    assert_eq!(provider.label(), "mock");
    assert_eq!(provider.model(), Some("mock-1"));
    assert_eq!(provider.direct_response_source(), "runtime-mock");
    assert_eq!(
        provider.tool_loop_response_source(),
        "runtime-mock-tool-loop"
    );
    assert!(provider.supports_images());
    provider.validate().unwrap();
}

#[test]
/// Verifies that extension reload re-reads config and manifests without resetting durable session state.
fn reload_extensions_rereads_config_without_resetting_sessions() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-runtime-ext-reload-{}",
        unix_timestamp_nanos()
    ));
    let _ = std::fs::remove_dir_all(&vela_home);
    std::fs::create_dir_all(vela_home.join("extensions")).unwrap();
    std::fs::write(
        vela_home.join("extensions").join("demo.yaml"),
        "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\nentry: extensions/demo-tool.wasm\ncapabilities:\n  - chat\n",
    )
    .unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "extensions:\n  entries:\n    demo:\n      enabled: false\n",
    )
    .unwrap();

    let (_, resolved_config) = vela_config::reload_config_snapshot(&vela_home, false).unwrap();
    let bootstrap = BootstrapReport {
        vela_home: vela_home.clone(),
        active_profile: None,
        loaded_env_paths: vec![],
        ignored_user_config: false,
        config_sources: vec![],
        resolved_config,
        persistence: vela_state::initialize_persistence(&vela_home).unwrap(),
        memory: vela_memory::initialize_memory(&vela_home).unwrap(),
        skills: vela_skills::initialize_skills(&vela_home).unwrap(),
        reviews: vela_review::initialize_reviews(&vela_home).unwrap(),
        extensions: vela_extensions::initialize_extensions(
            &vela_home,
            &vela_config::reload_config_snapshot(&vela_home, false)
                .unwrap()
                .1,
        )
        .unwrap(),
    };
    assert_eq!(bootstrap.extensions.activated_count, 0);
    assert_eq!(bootstrap.extensions.disabled_count, 1);

    let session = resolve_runtime_session(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("reload test".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    let before = current_session_summary(&bootstrap)
        .unwrap()
        .expect("session before reload");
    assert_eq!(before.id, session.session_id);

    std::fs::write(vela_home.join("config.yaml"), "extensions: {}\n").unwrap();
    let reloaded = reload_extensions(&bootstrap).unwrap();
    let after = current_session_summary(&bootstrap)
        .unwrap()
        .expect("session after reload");

    assert_eq!(reloaded.extensions.activated_count, 1);
    assert_eq!(reloaded.extensions.disabled_count, 0);
    assert!(reloaded.preserved_session);
    assert_eq!(before.id, after.id);
    assert_eq!(before.title, after.title);
    assert!(!reloaded.ownership_blocked);
    assert_eq!(reloaded.restart_required_drifts.len(), 0);
    assert_eq!(reloaded.ownership_baseline_source, "bootstrap-fallback");
    assert_eq!(
        reloaded.ownership_baseline_path,
        runtime_config_ownership_baseline_path(&vela_home)
    );
    assert!(reloaded
        .ownership_baseline_snapshot
        .contains("runtime.provider=null"));

    std::fs::write(
        vela_home.join("extensions").join("demo.yaml"),
        "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\ncapabilities:\n  - chat\n",
    )
    .unwrap();
    let failed = reload_extensions(&bootstrap).unwrap();
    let after_failed = current_session_summary(&bootstrap)
        .unwrap()
        .expect("session after failed reload");
    assert_eq!(failed.extensions.failed_count, 1);
    assert_eq!(failed.extensions.activated_count, 0);
    assert!(failed.preserved_session);
    assert_eq!(before.id, after_failed.id);
    assert_eq!(before.title, after_failed.title);
    assert!(!failed.ownership_blocked);

    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: changed\n  ollama_base_url: http://127.0.0.1:22555\nextensions: {}\n",
    )
    .unwrap();
    let drifted = reload_extensions(&bootstrap).unwrap();
    assert!(drifted.ownership_blocked);
    assert_eq!(drifted.restart_required_drifts.len(), 3);
    assert_eq!(drifted.ownership_baseline_source, "durable-baseline");
    assert_eq!(
        drifted.ownership_baseline_path,
        runtime_config_ownership_baseline_path(&vela_home)
    );
    assert!(drifted
        .ownership_baseline_snapshot
        .contains("runtime.provider=null"));
    assert!(drifted
        .summary_line()
        .contains("restart_required=runtime.provider@kernel-runtime,runtime.model@kernel-runtime,runtime.ollama_base_url@kernel-runtime ownership_blocked=true"));
    let expected_block_reason = format!(
        "extension reload blocked by kernel-owned runtime drift: runtime.provider@kernel-runtime, runtime.model@kernel-runtime, runtime.ollama_base_url@kernel-runtime (restart vela with the updated config to refresh the ownership baseline at {})",
        runtime_config_ownership_baseline_path(&vela_home).display()
    );
    assert_eq!(
        drifted.ownership_block_reason().as_deref(),
        Some(expected_block_reason.as_str())
    );
    assert!(drifted.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.provider"
            && item.owner == "kernel-runtime"
            && item.detail == "provider backend changes remain restart-only during extension reload"
            && item.previous_value == "null"
            && item.reloaded_value == "\"mock\""
    }));
    assert!(drifted.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.model"
            && item.owner == "kernel-runtime"
            && item.detail == "runtime model changes remain restart-only during extension reload"
            && item.previous_value == "null"
            && item.reloaded_value == "\"changed\""
    }));
    assert!(drifted.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.ollama_base_url"
            && item.owner == "kernel-runtime"
            && item.detail
                == "provider transport endpoint changes remain restart-only during extension reload"
            && item.previous_value == "null"
            && item.reloaded_value == "\"http://127.0.0.1:22555\""
    }));

    let _ = std::fs::remove_dir_all(&vela_home);
}

#[test]
/// Verifies that legacy slot registries are upgraded with the current default provider experiment slots.
fn setup_backend_evals_upgrades_legacy_slot_registry() {
    let bootstrap = test_bootstrap("eval-slot-upgrade");
    let evals_dir = bootstrap.vela_home.join("evals");
    std::fs::create_dir_all(&evals_dir).unwrap();
    let slots_path = evals_dir.join("slots.json");
    std::fs::write(
        &slots_path,
        serde_json::to_string_pretty(&vec![BackendExperimentSlotRecord {
            id: "ternary-preview".to_string(),
            status: "bounded-preview".to_string(),
            strategy: "shadow-routing".to_string(),
            summary: Some("legacy slot".to_string()),
            hypothesis: Some("legacy hypothesis".to_string()),
            default_prompt: "legacy prompt".to_string(),
            allowed_backends: vec!["mock".to_string()],
        }])
        .unwrap(),
    )
    .unwrap();

    let setup = setup_backend_evals(&bootstrap).unwrap();
    assert_eq!(setup.slot_count, 5);

    let slots = list_backend_experiment_slots(&bootstrap).unwrap();
    assert_eq!(slots.len(), 5);
    assert!(slots.iter().any(|slot| slot.id == "ternary-preview"));
    assert!(slots.iter().any(|slot| {
        slot.id == "ternary-preview"
            && slot
                .allowed_backends
                .iter()
                .any(|backend| backend == "embedded")
    }));
    assert!(slots.iter().any(|slot| slot.id == "sparse-routing-preview"));
    assert!(slots.iter().any(|slot| slot.id == "local-first-replay"));
    assert!(slots.iter().any(|slot| slot.id == "adapter-intake-gate"));
    assert!(slots.iter().any(|slot| slot.id == "capability-parity-scan"));
    assert!(slots.iter().any(|slot| {
        slot.id == "sparse-routing-preview"
            && slot
                .allowed_backends
                .iter()
                .any(|backend| backend == "embedded")
    }));
    assert!(slots.iter().any(|slot| {
        slot.id == "local-first-replay"
            && slot
                .allowed_backends
                .iter()
                .any(|backend| backend == "embedded")
    }));
    assert!(slots.iter().any(|slot| {
        slot.id == "adapter-intake-gate"
            && slot
                .allowed_backends
                .iter()
                .any(|backend| backend == "embedded")
    }));
    assert!(slots.iter().any(|slot| {
        slot.id == "capability-parity-scan"
            && slot
                .allowed_backends
                .iter()
                .any(|backend| backend == "embedded")
    }));

    let persisted = std::fs::read_to_string(&slots_path).unwrap();
    assert!(persisted.contains("sparse-routing-preview"));
    assert!(persisted.contains("local-first-replay"));
    assert!(persisted.contains("adapter-intake-gate"));
    assert!(persisted.contains("capability-parity-scan"));

    let _ = std::fs::remove_dir_all(&bootstrap.vela_home);
}

#[test]
/// Verifies that experiment slot inspection surfaces latest durable slot evidence in both summary groups and per-backend detail.
fn backend_experiment_slot_inspection_surfaces_latest_eval_evidence_details() {
    let mut bootstrap = test_bootstrap("eval-slot-inspection");
    bootstrap.resolved_config = ResolvedConfig {
        runtime_provider: Some("mock".to_string()),
        runtime_model: Some("mock-1".to_string()),
        ..ResolvedConfig::default()
    };

    let run = run_backend_eval_slot(
        &bootstrap,
        "capability-parity-scan",
        &["mock".to_string(), "llamacpp".to_string()],
        Some("mock-2"),
    )
    .unwrap();
    assert_eq!(run.record.results.len(), 2);
    assert_eq!(
        run.record.experiment_slot.as_deref(),
        Some("capability-parity-scan")
    );

    let inspection = get_backend_experiment_slot_inspection(&bootstrap, "capability-parity-scan")
        .unwrap()
        .expect("slot inspection");
    assert_eq!(inspection.latest_eval_id, Some(run.record.id.clone()));
    assert_eq!(inspection.latest_eval_backends, vec!["mock", "llamacpp"]);
    assert_eq!(inspection.latest_eval_passed_backends, vec!["mock"]);
    assert_eq!(inspection.latest_eval_failed_backends, vec!["llamacpp"]);
    assert_eq!(inspection.latest_eval_result_count, 2);
    assert!(inspection
        .latest_eval_capability_groups
        .iter()
        .any(|group| group
            .contains("mock=>text=true tool_loop=true reflection_retry=true images=true")));
    assert!(inspection
        .latest_eval_capability_groups
        .iter()
        .any(|group| group
            .contains("llamacpp=>text=true tool_loop=true reflection_retry=true images=false")));
    assert!(inspection
        .latest_backend_evidence
        .iter()
        .any(|item| item == "mock:passed@in-process source=runtime-mock model=mock-2"));
    assert!(inspection
        .latest_backend_evidence
        .iter()
        .any(|item| item == "llamacpp:failed@http-json source=none model=mock-2"));

    let _ = std::fs::remove_dir_all(&bootstrap.vela_home);
}

#[test]
/// Verifies that runtime ownership status surfaces pending restart-required drift before an extension reload is attempted.
fn runtime_ownership_status_surfaces_pending_restart_required_drift() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-runtime-ownership-status-{}",
        unix_timestamp_nanos()
    ));
    let _ = std::fs::remove_dir_all(&vela_home);
    std::fs::create_dir_all(vela_home.join("extensions")).unwrap();
    std::fs::write(
        vela_home.join("extensions").join("demo.yaml"),
        "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\nentry: extensions/demo-tool.wasm\ncapabilities:\n  - chat\n",
    )
    .unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: ollama\n  model: gemma3:4b\n  ollama_base_url: http://127.0.0.1:11434\nextensions: {}\n",
    )
    .unwrap();

    let (_, old_resolved_config) = vela_config::reload_config_snapshot(&vela_home, false).unwrap();
    ensure_runtime_config_ownership_baseline(&vela_home, &old_resolved_config).unwrap();

    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: changed\n  ollama_base_url: http://127.0.0.1:22555\nextensions: {}\n",
    )
    .unwrap();
    let (_, new_resolved_config) = vela_config::reload_config_snapshot(&vela_home, false).unwrap();
    let bootstrap = BootstrapReport {
        vela_home: vela_home.clone(),
        active_profile: None,
        loaded_env_paths: vec![],
        ignored_user_config: false,
        config_sources: vec![],
        resolved_config: new_resolved_config,
        persistence: vela_state::initialize_persistence(&vela_home).unwrap(),
        memory: vela_memory::initialize_memory(&vela_home).unwrap(),
        skills: vela_skills::initialize_skills(&vela_home).unwrap(),
        reviews: vela_review::initialize_reviews(&vela_home).unwrap(),
        extensions: vela_extensions::initialize_extensions(
            &vela_home,
            &vela_config::reload_config_snapshot(&vela_home, false)
                .unwrap()
                .1,
        )
        .unwrap(),
    };

    let ownership = inspect_runtime_ownership_status(&bootstrap).unwrap();
    assert!(ownership.ownership_blocked);
    assert_eq!(ownership.ownership_baseline_source, "durable-baseline");
    assert_eq!(
        ownership.ownership_baseline_path,
        runtime_config_ownership_baseline_path(&vela_home)
    );
    assert!(ownership
        .ownership_baseline_snapshot
        .contains("runtime.provider=\"ollama\""));
    assert_eq!(ownership.restart_required_drifts.len(), 3);
    assert!(ownership.summary_line().contains("status=restart-required"));
    assert!(ownership.summary_line().contains(
        "restart_required=runtime.provider@kernel-runtime,runtime.model@kernel-runtime,runtime.ollama_base_url@kernel-runtime"
    ));
    assert!(ownership.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.provider"
            && item.previous_value == "\"ollama\""
            && item.reloaded_value == "\"mock\""
    }));

    let _ = std::fs::remove_dir_all(&vela_home);
}

#[test]
/// Verifies that extension reload compares against a durable ownership baseline across fresh bootstraps.
fn reload_extensions_uses_durable_ownership_baseline_across_bootstraps() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-runtime-ext-reload-baseline-{}",
        unix_timestamp_nanos()
    ));
    let _ = std::fs::remove_dir_all(&vela_home);
    std::fs::create_dir_all(vela_home.join("extensions")).unwrap();
    std::fs::write(
        vela_home.join("extensions").join("demo.yaml"),
        "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\nentry: extensions/demo-tool.wasm\ncapabilities:\n  - chat\n",
    )
    .unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: ollama\n  model: gemma3:4b\n  ollama_base_url: http://127.0.0.1:11434\nextensions: {}\n",
    )
    .unwrap();

    let (_, old_resolved_config) = vela_config::reload_config_snapshot(&vela_home, false).unwrap();
    ensure_runtime_config_ownership_baseline(&vela_home, &old_resolved_config).unwrap();
    let baseline_path = runtime_config_ownership_baseline_path(&vela_home);
    assert!(baseline_path.is_file());
    let baseline_before = std::fs::read_to_string(&baseline_path).unwrap();
    assert!(baseline_before.contains("\"runtime_provider\": \"ollama\""));

    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: changed\n  ollama_base_url: http://127.0.0.1:22555\nextensions: {}\n",
    )
    .unwrap();

    let (_, new_resolved_config) = vela_config::reload_config_snapshot(&vela_home, false).unwrap();
    let second = BootstrapReport {
        vela_home: vela_home.clone(),
        active_profile: None,
        loaded_env_paths: vec![],
        ignored_user_config: false,
        config_sources: vec![],
        resolved_config: new_resolved_config,
        persistence: vela_state::initialize_persistence(&vela_home).unwrap(),
        memory: vela_memory::initialize_memory(&vela_home).unwrap(),
        skills: vela_skills::initialize_skills(&vela_home).unwrap(),
        reviews: vela_review::initialize_reviews(&vela_home).unwrap(),
        extensions: vela_extensions::initialize_extensions(
            &vela_home,
            &vela_config::reload_config_snapshot(&vela_home, false)
                .unwrap()
                .1,
        )
        .unwrap(),
    };
    let drifted = reload_extensions(&second).unwrap();
    assert!(drifted.ownership_blocked);
    assert_eq!(drifted.restart_required_drifts.len(), 3);
    assert_eq!(drifted.ownership_baseline_source, "durable-baseline");
    assert_eq!(drifted.ownership_baseline_path, baseline_path);
    assert!(drifted.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.provider"
            && item.owner == "kernel-runtime"
            && item.previous_value == "\"ollama\""
            && item.reloaded_value == "\"mock\""
    }));
    assert!(drifted.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.model"
            && item.owner == "kernel-runtime"
            && item.previous_value == "\"gemma3:4b\""
            && item.reloaded_value == "\"changed\""
    }));
    assert!(drifted.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.ollama_base_url"
            && item.owner == "kernel-runtime"
            && item.previous_value == "\"http://127.0.0.1:11434\""
            && item.reloaded_value == "\"http://127.0.0.1:22555\""
    }));
    let baseline_after = std::fs::read_to_string(&baseline_path).unwrap();
    assert!(baseline_after.contains("\"runtime_provider\": \"ollama\""));

    let _ = std::fs::remove_dir_all(&vela_home);
}

struct MockOllamaExchange<'a> {
    response_body: &'a str,
    expected_model: &'a str,
    prompt_fragment: &'a str,
    expected_image_base64: Option<&'a str>,
}

fn read_mock_http_request(stream: &mut std::net::TcpStream) -> String {
    use std::io::Read;

    let mut request_bytes = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end;
    let expected_total_len;
    loop {
        let read = stream.read(&mut buf).expect("read mock Ollama request");
        assert!(
            read > 0,
            "mock Ollama request closed before full payload arrived"
        );
        request_bytes.extend_from_slice(&buf[..read]);
        if let Some(end) = request_bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let end = end + 4;
            let head = String::from_utf8_lossy(&request_bytes[..end]).into_owned();
            let content_length = head
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Content-Length: ")
                        .or_else(|| line.strip_prefix("content-length: "))
                })
                .expect("Content-Length header")
                .trim()
                .parse::<usize>()
                .expect("parse Content-Length");
            header_end = end;
            expected_total_len = header_end + content_length;
            break;
        }
    }
    while request_bytes.len() < expected_total_len {
        let read = stream
            .read(&mut buf)
            .expect("read mock Ollama request body");
        assert!(read > 0, "mock Ollama request closed before body finished");
        request_bytes.extend_from_slice(&buf[..read]);
    }
    String::from_utf8_lossy(&request_bytes[..expected_total_len]).into_owned()
}

fn assert_mock_ollama_request(request: &str, exchange: &MockOllamaExchange<'_>) {
    let (head, body_text) = request.split_once("\r\n\r\n").expect("split HTTP request");
    let request_line = head.lines().next().expect("HTTP request line");
    assert!(request_line.starts_with("POST /api/generate HTTP/1.1"));
    let payload_json: serde_json::Value =
        serde_json::from_str(body_text).expect("decode request body");
    assert_eq!(
        payload_json.get("model").and_then(|v| v.as_str()),
        Some(exchange.expected_model)
    );
    assert_eq!(
        payload_json.get("stream").and_then(|v| v.as_bool()),
        Some(false)
    );
    let prompt = payload_json
        .get("prompt")
        .and_then(|v| v.as_str())
        .expect("prompt field");
    assert!(prompt.contains(exchange.prompt_fragment));
    let images = payload_json.get("images").and_then(|v| v.as_array());
    if let Some(expected_image_base64) = exchange.expected_image_base64 {
        let images = images.expect("images field");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].as_str(), Some(expected_image_base64));
    } else {
        assert!(
            payload_json.get("images").is_none(),
            "images field should be absent when no image is expected"
        );
    }
}

fn spawn_mock_ollama_sequence(
    exchanges: Vec<MockOllamaExchange<'static>>,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        for exchange in exchanges {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_mock_http_request(&mut stream);
            assert_mock_ollama_request(&request, &exchange);
            let payload = serde_json::json!({ "response": exchange.response_body }).to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            stream.write_all(reply.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
    });
    (addr, handle)
}

fn spawn_mock_ollama(
    response_body: &'static str,
    expected_model: &'static str,
    prompt_fragment: &'static str,
    expected_image_base64: Option<&'static str>,
) -> (String, std::thread::JoinHandle<()>) {
    spawn_mock_ollama_sequence(vec![MockOllamaExchange {
        response_body,
        expected_model,
        prompt_fragment,
        expected_image_base64,
    }])
}

#[test]
/// Verifies that scheduler registrations persist and duplicate pending jobs are rejected.
fn scheduler_jobs_persist_and_dedupe() {
    let bootstrap = test_bootstrap("scheduler-test");

    let first =
        add_scheduled_job(&bootstrap, "0 * * * *", "ping status", None, None, None).unwrap();
    let jobs = list_scheduled_jobs(&bootstrap).unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, first.id);

    let err =
        add_scheduled_job(&bootstrap, "0 * * * *", "ping status", None, None, None).unwrap_err();
    assert!(err.to_string().contains("already registered"));

    let fetched = get_scheduled_job(&bootstrap, &first.id).unwrap();
    assert_eq!(fetched.task, "ping status");

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies gateway restart continuity without duplicating the bootstrap message.
fn gateway_start_resumes_same_session_without_duplicate_bootstrap_message() {
    let bootstrap = test_bootstrap("gateway-resume");

    let first = start_gateway(&bootstrap).unwrap();
    let first_summary = current_command_session_summary(&bootstrap, "gateway")
        .unwrap()
        .expect("initial gateway session summary");
    let second = start_gateway(&bootstrap).unwrap();

    assert_eq!(first.session.session_id, second.session.session_id);
    let summary = current_command_session_summary(&bootstrap, "gateway")
        .unwrap()
        .expect("gateway session summary");
    assert_eq!(first_summary.message_count, 1);
    assert_eq!(summary.message_count, 1);
    assert_eq!(summary.event_count, first_summary.event_count + 2);

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies gateway webhook delivery persists an outbox record and logs durable session activity.
fn gateway_webhook_delivery_persists_outbox_and_logs_session_activity() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let bootstrap = test_bootstrap("gateway-webhook-success");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/hook",
        listener.local_addr().expect("webhook listener addr")
    );
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept webhook request");
        let mut request_bytes = Vec::new();
        let mut buf = [0u8; 4096];
        let header_end;
        let expected_total_len;
        loop {
            let read = stream.read(&mut buf).expect("read webhook request");
            assert!(read > 0, "webhook request closed before headers arrived");
            request_bytes.extend_from_slice(&buf[..read]);
            if let Some(end) = request_bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let end = end + 4;
                let head = String::from_utf8_lossy(&request_bytes[..end]).into_owned();
                let content_length = head
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("Content-Length: ")
                            .or_else(|| line.strip_prefix("content-length: "))
                    })
                    .expect("Content-Length header")
                    .trim()
                    .parse::<usize>()
                    .expect("parse Content-Length");
                header_end = end;
                expected_total_len = header_end + content_length;
                break;
            }
        }
        while request_bytes.len() < expected_total_len {
            let read = stream.read(&mut buf).expect("read webhook request body");
            assert!(read > 0, "webhook request closed before body finished");
            request_bytes.extend_from_slice(&buf[..read]);
        }
        let request = String::from_utf8_lossy(&request_bytes[..expected_total_len]).into_owned();
        let (head, body_text) = request
            .split_once("\r\n\r\n")
            .expect("split HTTP headers/body");
        assert!(
            head.lines()
                .next()
                .expect("request line")
                .starts_with("POST /hook HTTP/1.1"),
            "unexpected request line: {head}"
        );
        let payload: serde_json::Value =
            serde_json::from_str(body_text).expect("decode webhook payload");
        assert_eq!(
            payload.get("event_type").and_then(|v| v.as_str()),
            Some("delivery.test")
        );
        assert_eq!(
            payload.get("payload").and_then(|v| v.as_str()),
            Some("ping gateway")
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .expect("write webhook response");
    });

    let report =
        deliver_gateway_webhook(&bootstrap, &url, "ping gateway", Some("delivery.test")).unwrap();
    assert_eq!(report.status_code, 200);
    assert!(report.outbox_record_path.is_file());
    let outbox = std::fs::read_to_string(&report.outbox_record_path).unwrap();
    assert!(outbox.contains("\"result\": \"delivered\""));
    assert!(outbox.contains("delivery.test"));
    assert!(outbox.contains("ping gateway"));

    let summary = current_command_session_summary(&bootstrap, "gateway")
        .unwrap()
        .expect("gateway session summary");
    assert!(summary.message_count >= 2);
    assert!(summary.event_count >= 2);

    handle.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies gateway webhook delivery records a failed attempt when the remote endpoint is unavailable.
fn gateway_webhook_delivery_records_failed_attempts() {
    let bootstrap = test_bootstrap("gateway-webhook-failure");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    drop(listener);

    let err = deliver_gateway_webhook(&bootstrap, &url, "ping gateway", Some("delivery.test"))
        .unwrap_err();
    assert!(!err.to_string().is_empty());

    let outbox_dir = bootstrap.vela_home.join("gateway").join("outbox");
    let record = std::fs::read_dir(&outbox_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .expect("failed outbox record");
    let outbox = std::fs::read_to_string(record).unwrap();
    assert!(outbox.contains("\"result\": \"failed\""));
    assert!(outbox.contains("delivery.test"));

    let summary = current_command_session_summary(&bootstrap, "gateway")
        .unwrap()
        .expect("gateway session summary");
    assert!(summary.event_count >= 2);

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that bounded subagent delegation persists and duplicate pending requests are rejected.
fn subagent_delegation_persists_and_dedupes_pending_requests() {
    let bootstrap = test_bootstrap("agents-delegation");

    let first = request_subagent_delegation(
        &bootstrap,
        "researcher",
        "Investigate provider routing",
        Some("bounded follow-up"),
    )
    .unwrap();
    assert_eq!(first.record.role, "researcher");
    assert_eq!(first.record.status, "pending");

    let records = list_subagent_delegations(&bootstrap).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, first.record.id);

    let fetched = get_subagent_delegation(&bootstrap, &first.record.id)
        .unwrap()
        .expect("delegation record");
    assert_eq!(fetched.task, "Investigate provider routing");
    assert_eq!(fetched.note.as_deref(), Some("bounded follow-up"));

    let summary = current_command_session_summary(&bootstrap, "agents")
        .unwrap()
        .expect("agents session summary");
    assert!(summary.message_count >= 1);
    assert!(summary.event_count >= 1);

    let err = request_subagent_delegation(
        &bootstrap,
        "researcher",
        "Investigate provider routing",
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("already pending"));

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that bounded MCP bridge requests persist and duplicate pending requests are rejected.
fn mcp_bridge_requests_persist_and_dedupe_pending_requests() {
    let bootstrap = test_bootstrap("mcp-bridge");

    let first = request_mcp_bridge_call(
        &bootstrap,
        "memory",
        "list_tools",
        "{}",
        Some("bounded bridge request"),
    )
    .unwrap();
    assert_eq!(first.record.server, "memory");
    assert_eq!(first.record.tool, "list_tools");
    assert_eq!(first.record.status, "pending");

    let records = list_mcp_bridge_calls(&bootstrap).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, first.record.id);

    let fetched = get_mcp_bridge_call(&bootstrap, &first.record.id)
        .unwrap()
        .expect("mcp bridge record");
    assert_eq!(fetched.payload, "{}");
    assert_eq!(fetched.note.as_deref(), Some("bounded bridge request"));

    let summary = current_command_session_summary(&bootstrap, "mcp")
        .unwrap()
        .expect("mcp session summary");
    assert!(summary.message_count >= 1);
    assert!(summary.event_count >= 1);

    let err = request_mcp_bridge_call(&bootstrap, "memory", "list_tools", "{}", None).unwrap_err();
    assert!(err.to_string().contains("already pending"));

    let invalid =
        request_mcp_bridge_call(&bootstrap, "memory", "list_tools", "not-json", None).unwrap_err();
    assert!(invalid.to_string().contains("must be valid JSON"));

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies scheduler restart continuity while preserving registered durable jobs.
fn scheduler_start_resumes_same_session_and_preserves_registered_jobs() {
    let bootstrap = test_bootstrap("scheduler-resume");

    let first = start_scheduler(&bootstrap).unwrap();
    let first_summary = current_command_session_summary(&bootstrap, "cron")
        .unwrap()
        .expect("initial cron session summary");
    let added = add_scheduled_job(
        &bootstrap,
        "*/5 * * * *",
        "ping status",
        Some("test"),
        None,
        None,
    )
    .unwrap();
    let setup = setup_scheduler(&bootstrap).unwrap();
    let lock = acquire_scheduler_jobs_lock(&setup.jobs_path).unwrap();
    let mut jobs = load_scheduler_jobs(&setup.jobs_path).unwrap();
    let job = jobs
        .iter_mut()
        .find(|job| job.id == added.id)
        .expect("scheduler job");
    job.next_run_at = unix_timestamp() - 1;
    save_scheduler_jobs(&setup.jobs_path, &jobs).unwrap();
    drop(lock);
    let second = start_scheduler(&bootstrap).unwrap();

    assert_eq!(first.session.session_id, second.session.session_id);
    assert_eq!(second.setup.job_count, 1);
    assert_eq!(second.executed_job_count, 1);
    let summary = current_command_session_summary(&bootstrap, "cron")
        .unwrap()
        .expect("cron session summary");
    assert!(summary.message_count >= first_summary.message_count + 2);
    assert!(summary.event_count >= first_summary.event_count + 4);

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies scheduler execution reschedules completed jobs and recovers stale running jobs safely.
fn scheduler_executes_and_recovers_jobs() {
    let bootstrap = test_bootstrap("scheduler-exec-recover");
    let added = add_scheduled_job(
        &bootstrap,
        "* * * * *",
        "ping status",
        Some("test"),
        None,
        None,
    )
    .unwrap();
    let setup = setup_scheduler(&bootstrap).unwrap();
    let lock = acquire_scheduler_jobs_lock(&setup.jobs_path).unwrap();
    let mut jobs = load_scheduler_jobs(&setup.jobs_path).unwrap();
    let job = jobs
        .iter_mut()
        .find(|job| job.id == added.id)
        .expect("scheduler job");
    job.next_run_at = unix_timestamp() - 1;
    save_scheduler_jobs(&setup.jobs_path, &jobs).unwrap();
    drop(lock);

    let first = start_scheduler(&bootstrap).unwrap();
    assert_eq!(first.executed_job_count, 1);
    assert_eq!(first.recovered_job_count, 0);
    assert_eq!(first.failed_job_count, 0);
    let first_job = get_scheduled_job(&bootstrap, &added.id).unwrap();
    assert_eq!(first_job.status, "pending");
    assert_eq!(first_job.run_count, 1);
    assert_eq!(first_job.last_outcome.as_deref(), Some("completed"));
    assert_eq!(
        first_job.last_progression.as_deref(),
        Some("completed-rescheduled")
    );
    assert!(first_job.next_run_at > first_job.created_at);

    let setup = setup_scheduler(&bootstrap).unwrap();
    let lock = acquire_scheduler_jobs_lock(&setup.jobs_path).unwrap();
    let mut jobs = load_scheduler_jobs(&setup.jobs_path).unwrap();
    let job = jobs
        .iter_mut()
        .find(|job| job.id == added.id)
        .expect("scheduler job");
    let stale_started_at = unix_timestamp() - SCHEDULER_RECOVERY_LEASE_SECONDS - 1;
    job.status = "running".to_string();
    job.last_started_at = Some(stale_started_at);
    job.execution_token = Some("stale-attempt".to_string());
    job.lease_expires_at = Some(unix_timestamp() - 1);
    job.next_run_at = unix_timestamp() - 1;
    save_scheduler_jobs(&setup.jobs_path, &jobs).unwrap();
    drop(lock);

    let second = start_scheduler(&bootstrap).unwrap();
    assert_eq!(second.executed_job_count, 1);
    assert_eq!(second.recovered_job_count, 1);
    assert_eq!(second.failed_job_count, 0);
    let recovered_job = get_scheduled_job(&bootstrap, &added.id).unwrap();
    assert_eq!(recovered_job.status, "pending");
    assert_eq!(recovered_job.run_count, 2);
    assert_eq!(recovered_job.recovery_count, 1);
    assert_eq!(recovered_job.last_outcome.as_deref(), Some("completed"));
    assert_eq!(
        recovered_job.last_progression.as_deref(),
        Some("completed-rescheduled")
    );
    assert!(recovered_job.last_recovered_at.is_some());

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies scheduler job outcomes can be delivered through the gateway webhook path.
fn scheduler_job_delivery_uses_gateway_webhook() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let bootstrap = test_bootstrap("scheduler-delivery-success");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept scheduler webhook request");
        let mut request_bytes = Vec::new();
        let mut buf = [0u8; 4096];
        let header_end;
        let expected_total_len;
        loop {
            let read = stream
                .read(&mut buf)
                .expect("read scheduler webhook request");
            assert!(
                read > 0,
                "scheduler webhook request closed before headers arrived"
            );
            request_bytes.extend_from_slice(&buf[..read]);
            if let Some(end) = request_bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let end = end + 4;
                let head = String::from_utf8_lossy(&request_bytes[..end]).into_owned();
                let content_length = head
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("Content-Length: ")
                            .or_else(|| line.strip_prefix("content-length: "))
                    })
                    .expect("Content-Length header")
                    .trim()
                    .parse::<usize>()
                    .expect("parse Content-Length");
                header_end = end;
                expected_total_len = header_end + content_length;
                break;
            }
        }
        while request_bytes.len() < expected_total_len {
            let read = stream
                .read(&mut buf)
                .expect("read scheduler webhook request body");
            assert!(
                read > 0,
                "scheduler webhook request closed before body finished"
            );
            request_bytes.extend_from_slice(&buf[..read]);
        }
        let request = String::from_utf8_lossy(&request_bytes[..expected_total_len]).into_owned();
        let (_, body_text) = request.split_once("\r\n\r\n").expect("split headers/body");
        let payload: serde_json::Value = serde_json::from_str(body_text).expect("decode payload");
        assert_eq!(
            payload.get("event_type").and_then(|v| v.as_str()),
            Some("scheduler.job.outcome")
        );
        let nested: serde_json::Value = serde_json::from_str(
            payload
                .get("payload")
                .and_then(|v| v.as_str())
                .expect("nested payload"),
        )
        .expect("decode nested payload");
        assert_eq!(
            nested.get("outcome").and_then(|v| v.as_str()),
            Some("completed")
        );
        assert_eq!(
            nested.get("task").and_then(|v| v.as_str()),
            Some("ping status")
        );
        let reply = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";
        stream.write_all(reply.as_bytes()).unwrap();
        stream.flush().unwrap();
    });

    let added = add_scheduled_job(
        &bootstrap,
        "* * * * *",
        "ping status",
        Some("test"),
        Some(&url),
        Some("scheduler.job.outcome"),
    )
    .unwrap();
    let setup = setup_scheduler(&bootstrap).unwrap();
    let lock = acquire_scheduler_jobs_lock(&setup.jobs_path).unwrap();
    let mut jobs = load_scheduler_jobs(&setup.jobs_path).unwrap();
    let job = jobs
        .iter_mut()
        .find(|job| job.id == added.id)
        .expect("scheduler job");
    job.next_run_at = unix_timestamp() - 1;
    save_scheduler_jobs(&setup.jobs_path, &jobs).unwrap();
    drop(lock);

    let start = start_scheduler(&bootstrap).unwrap();
    assert_eq!(start.executed_job_count, 1);
    let job = get_scheduled_job(&bootstrap, &added.id).unwrap();
    assert_eq!(job.last_delivery_outcome.as_deref(), Some("delivered"));
    assert!(job.last_delivery_at.is_some());
    assert!(job.last_delivery_error.is_none());

    let outbox_dir = bootstrap.vela_home.join("gateway").join("outbox");
    let outbox_record = std::fs::read_dir(&outbox_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .expect("scheduler delivery outbox record");
    let outbox = std::fs::read_to_string(outbox_record).unwrap();
    assert!(outbox.contains("scheduler.job.outcome"));
    assert!(outbox.contains("\"result\": \"delivered\""));

    handle.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies scheduler delivery failures are recorded without losing the completed job outcome.
fn scheduler_job_delivery_failures_are_recorded() {
    let bootstrap = test_bootstrap("scheduler-delivery-failure");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    drop(listener);

    let added = add_scheduled_job(
        &bootstrap,
        "* * * * *",
        "ping status",
        Some("test"),
        Some(&url),
        Some("scheduler.job.outcome"),
    )
    .unwrap();
    let setup = setup_scheduler(&bootstrap).unwrap();
    let lock = acquire_scheduler_jobs_lock(&setup.jobs_path).unwrap();
    let mut jobs = load_scheduler_jobs(&setup.jobs_path).unwrap();
    let job = jobs
        .iter_mut()
        .find(|job| job.id == added.id)
        .expect("scheduler job");
    job.next_run_at = unix_timestamp() - 1;
    save_scheduler_jobs(&setup.jobs_path, &jobs).unwrap();
    drop(lock);

    let start = start_scheduler(&bootstrap).unwrap();
    assert_eq!(start.executed_job_count, 1);
    assert_eq!(start.failed_job_count, 0);

    let job = get_scheduled_job(&bootstrap, &added.id).unwrap();
    assert_eq!(job.last_outcome.as_deref(), Some("completed"));
    assert_eq!(job.last_delivery_outcome.as_deref(), Some("failed"));
    assert!(!job
        .last_delivery_error
        .as_deref()
        .unwrap_or_default()
        .is_empty());

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that a query executes a live assistant turn and can emit review candidates.
fn execute_chat_turn_appends_response_and_checkpoint_artifacts() {
    let bootstrap = test_bootstrap("chat-turn");
    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("please always use terse answers".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        true,
    )
    .unwrap();

    assert!(report
        .response
        .as_deref()
        .unwrap_or_default()
        .contains("Vela executed a local kernel turn"));
    assert!(report.turn_id.starts_with("turn-"));
    assert_eq!(report.lifecycle_phase_count, 4);
    assert_eq!(report.final_phase, "finish");
    assert_eq!(report.emitted_signal_count, 1);
    assert_eq!(report.generated_candidate_count, 1);
    let summary = current_session_summary(&bootstrap)
        .unwrap()
        .expect("chat session summary");
    assert_eq!(summary.message_count, 2);
    let inspection = inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("chat session inspection");
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| record.phase.as_str())
        .collect();
    assert_eq!(
        lifecycle,
        vec!["receive", "deliberate", "respond", "finish"]
    );
    assert!(list_review_candidates(&bootstrap).unwrap().len() >= 1);

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that image-only turns still append an assistant response.
fn execute_chat_turn_handles_image_only_requests() {
    let bootstrap = test_bootstrap("image-turn");
    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: false,
            query_text: None,
            image_present: true,
            image_path: Some("diagram.png".to_string()),
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert!(report
        .response
        .as_deref()
        .unwrap_or_default()
        .contains("Vela executed a local image turn"));
    let summary = current_session_summary(&bootstrap)
        .unwrap()
        .expect("image chat session summary");
    assert_eq!(summary.message_count, 2);

    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that configured Ollama execution is used for image chat turns.
fn execute_chat_turn_uses_ollama_provider_for_image_requests() {
    let (base_url, server) = spawn_mock_ollama(
        "Gemma inspected the image.",
        "gemma3:4b",
        "Please analyze the attached image",
        Some("ZmFrZS1wbmctYnl0ZXM="),
    );
    let mut bootstrap = test_bootstrap("ollama-image-turn");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
    let image_path = bootstrap.vela_home.join("diagram.png");
    std::fs::create_dir_all(&bootstrap.vela_home).unwrap();
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: false,
            query_text: None,
            image_present: true,
            image_path: Some(image_path.to_string_lossy().into_owned()),
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Gemma inspected the image.")
    );
    assert_eq!(report.response_source, "runtime-ollama");
    let inspection = inspect_latest_session(&bootstrap, 10)
        .unwrap()
        .expect("image session inspection");
    let assistant = inspection.messages.last().expect("assistant message");
    let metadata: serde_json::Value = serde_json::from_str(
        assistant
            .metadata_json
            .as_deref()
            .expect("assistant metadata"),
    )
    .expect("decode assistant metadata");
    assert_eq!(
        metadata.get("source").and_then(|v| v.as_str()),
        Some("runtime-ollama")
    );
    assert_eq!(
        metadata.get("provider").and_then(|v| v.as_str()),
        Some("ollama")
    );
    assert_eq!(
        metadata.get("model").and_then(|v| v.as_str()),
        Some("gemma3:4b")
    );
    assert_eq!(
        metadata
            .get("provider_capabilities")
            .and_then(|v| v.as_str()),
        Some("text=true tool_loop=true reflection_retry=true images=true")
    );
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that mixed text+image requests still forward the image payload to Ollama.
fn execute_chat_turn_routes_mixed_text_and_image_requests_through_ollama_image_path() {
    let (base_url, server) = spawn_mock_ollama(
        "Gemma handled both prompt and image.",
        "gemma3:4b",
        "what is happening in this image?",
        Some("ZmFrZS1wbmctYnl0ZXM="),
    );
    let mut bootstrap = test_bootstrap("ollama-mixed-turn");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
    let image_path = bootstrap.vela_home.join("diagram.png");
    std::fs::create_dir_all(&bootstrap.vela_home).unwrap();
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("what is happening in this image?".to_string()),
            image_present: true,
            image_path: Some(image_path.to_string_lossy().into_owned()),
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Gemma handled both prompt and image.")
    );
    assert_eq!(report.response_source, "runtime-ollama");
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that mixed text+image requests execute through the mock provider path and reflect the user query.
fn execute_chat_turn_routes_mixed_text_and_image_requests_through_mock_provider() {
    let mut bootstrap = test_bootstrap("mock-mixed-turn");
    bootstrap.resolved_config.runtime_provider = Some("mock".to_string());
    bootstrap.resolved_config.runtime_model = Some("mock-1".to_string());
    let image_path = bootstrap.vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("summarize the mock diagram".to_string()),
            image_present: true,
            image_path: Some(image_path.to_string_lossy().into_owned()),
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Mock provider inspected the image for request: summarize the mock diagram.")
    );
    assert_eq!(report.response_source, "runtime-mock");
    assert_eq!(report.response_provider.as_deref(), Some("mock"));
    assert_eq!(report.response_model.as_deref(), Some("mock-1"));
    assert_eq!(
        report.response_provider_capabilities.as_deref(),
        Some("text=true tool_loop=true reflection_retry=true images=true")
    );
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that configured Ollama execution is used for text chat turns.
fn execute_chat_turn_uses_ollama_provider_when_configured() {
    let (base_url, server) = spawn_mock_ollama("Gemma says hi.", "gemma3:4b", "hello there", None);
    let mut bootstrap = test_bootstrap("ollama-turn");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("hello there".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(report.response.as_deref(), Some("Gemma says hi."));
    assert_eq!(report.response_source, "runtime-ollama");
    assert_eq!(report.response_provider.as_deref(), Some("ollama"));
    assert_eq!(report.response_model.as_deref(), Some("gemma3:4b"));
    assert_eq!(
        report.response_provider_capabilities.as_deref(),
        Some("text=true tool_loop=true reflection_retry=true images=true")
    );
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that configured mock execution is used for text chat turns.
fn execute_chat_turn_uses_mock_provider_when_configured() {
    let mut bootstrap = test_bootstrap("mock-turn");
    bootstrap.resolved_config.runtime_provider = Some("mock".to_string());
    bootstrap.resolved_config.runtime_model = Some("mock-1".to_string());

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("hello from mock".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(report.response.as_deref(), Some("Mock provider says hi."));
    assert_eq!(report.response_source, "runtime-mock");
    assert_eq!(report.response_provider.as_deref(), Some("mock"));
    assert_eq!(report.response_model.as_deref(), Some("mock-1"));
    assert_eq!(
        report.response_provider_capabilities.as_deref(),
        Some("text=true tool_loop=true reflection_retry=true images=true")
    );
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that mock provider image requests execute through the provider path with explicit route details.
fn execute_chat_turn_uses_mock_provider_for_image_requests() {
    let mut bootstrap = test_bootstrap("mock-image-fallback");
    bootstrap.resolved_config.runtime_provider = Some("mock".to_string());
    bootstrap.resolved_config.runtime_model = Some("mock-1".to_string());
    let image_path = bootstrap.vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: false,
            query_text: None,
            image_present: true,
            image_path: Some(image_path.display().to_string()),
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Mock provider inspected the image.")
    );
    assert_eq!(report.response_source, "runtime-mock");
    assert_eq!(report.response_provider.as_deref(), Some("mock"));
    assert_eq!(report.response_model.as_deref(), Some("mock-1"));
    assert_eq!(
        report.response_provider_capabilities.as_deref(),
        Some("text=true tool_loop=true reflection_retry=true images=true")
    );
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that a configured provider turn can retrieve targeted memory, session, and skill context through runtime tools.
fn execute_chat_turn_retrieves_targeted_internal_context() {
    let (base_url, server) = spawn_mock_ollama_sequence(vec![
        MockOllamaExchange {
            response_body: r#"{"tool":"view_memory","target":"user"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "retrieve targeted context",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"search_session_history","query":"retrieve targeted context","limit":2}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "Tool result for view_memory:user:",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"view_skill","name":"deploy-staging"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "Tool result for search_session_history:retrieve targeted context:",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: "Context-aware final answer.",
            expected_model: "gemma3:4b",
            prompt_fragment: "Tool result for view_skill:deploy-staging:",
            expected_image_base64: None,
        },
    ]);
    let mut bootstrap = test_bootstrap("ollama-context-tools");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
    vela_memory::add_memory_entry(
        &bootstrap.vela_home,
        vela_memory::MemoryTarget::User,
        "Prefers terse answers.",
    )
    .unwrap();
    std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        bootstrap
            .vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploy staging safely.",
    )
    .unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("retrieve targeted context".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Context-aware final answer.")
    );
    assert_eq!(report.response_source, "runtime-ollama-tool-loop");
    let inspection = inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("context tool inspection");
    assert_eq!(inspection.messages[1].content, "view_memory:user");
    assert!(inspection.messages[2]
        .content
        .contains("Prefers terse answers."));
    assert_eq!(
        inspection.messages[3].content,
        "search_session_history:retrieve targeted context"
    );
    assert!(inspection.messages[4].content.contains("retrieve"));
    assert_eq!(inspection.messages[5].content, "view_skill:deploy-staging");
    assert!(inspection.messages[6]
        .content
        .contains("Deploy staging safely."));
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that a configured provider turn can execute a bounded multi-step local tool sequence.
fn execute_chat_turn_runs_first_runtime_tool_loop() {
    let (base_url, server) = spawn_mock_ollama_sequence(vec![
        MockOllamaExchange {
            response_body: r#"{"tool":"memory_snapshot"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "need the tool loop",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"list_skills"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "Completed tool step 1 of 3.\nTool result for memory_snapshot:",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: "Tool-informed final answer.",
            expected_model: "gemma3:4b",
            prompt_fragment:
                "Completed tool step 2 of 3.\nTool result for list_skills:\ndeploy-staging",
            expected_image_base64: None,
        },
    ]);
    let mut bootstrap = test_bootstrap("ollama-tool-loop");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
    std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        bootstrap
            .vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("need the tool loop".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Tool-informed final answer.")
    );
    assert_eq!(report.response_source, "runtime-ollama-tool-loop");
    assert_eq!(report.lifecycle_phase_count, 8);
    assert_eq!(report.final_phase, "finish");
    let inspection = inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("tool loop inspection");
    assert_eq!(inspection.messages.len(), 6);
    assert_eq!(inspection.messages[1].role, "tool-request");
    assert_eq!(inspection.messages[1].content, "memory_snapshot");
    assert_eq!(inspection.messages[2].role, "tool-result");
    assert!(!inspection.messages[2].content.trim().is_empty());
    let first_tool_result_metadata: serde_json::Value = serde_json::from_str(
        inspection.messages[2]
            .metadata_json
            .as_deref()
            .expect("first tool-result metadata"),
    )
    .expect("decode first tool-result metadata");
    assert_eq!(
        first_tool_result_metadata
            .get("request")
            .and_then(|v| v.get("tool"))
            .and_then(|v| v.as_str()),
        Some("memory_snapshot")
    );
    assert_eq!(
        first_tool_result_metadata
            .get("step")
            .and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(inspection.messages[3].role, "tool-request");
    assert_eq!(inspection.messages[3].content, "list_skills");
    assert_eq!(inspection.messages[4].role, "tool-result");
    assert!(inspection.messages[4].content.contains("deploy-staging"));
    assert_eq!(
        inspection
            .events
            .iter()
            .filter(|event| event.event_type == "runtime_tool_requested")
            .count(),
        2
    );
    assert_eq!(
        inspection
            .events
            .iter()
            .filter(|event| event.event_type == "runtime_tool_completed")
            .count(),
        2
    );
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| (record.phase.as_str(), record.step))
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            ("receive", None),
            ("deliberate", None),
            ("tool-request", Some(1)),
            ("tool-result", Some(1)),
            ("tool-request", Some(2)),
            ("tool-result", Some(2)),
            ("respond", None),
            ("finish", None),
        ]
    );
    let assistant = inspection.messages.last().expect("assistant message");
    let metadata: serde_json::Value = serde_json::from_str(
        assistant
            .metadata_json
            .as_deref()
            .expect("assistant metadata"),
    )
    .expect("decode assistant metadata");
    assert_eq!(
        metadata.get("source").and_then(|v| v.as_str()),
        Some("runtime-ollama-tool-loop")
    );
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that the embedded provider can execute the bounded local tool loop through the existing runtime path.
fn execute_chat_turn_runs_embedded_provider_tool_loop() {
    let mut bootstrap = test_bootstrap("embedded-tool-loop");
    bootstrap.resolved_config.runtime_provider = Some("embedded".to_string());
    let model_path = bootstrap.vela_home.join("models").join("gemma3.gguf");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"stub model").unwrap();
    bootstrap.resolved_config.runtime_embedded_model_path =
        Some(model_path.to_string_lossy().into_owned());
    std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        bootstrap
            .vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("need the tool loop".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Embedded tool-informed final answer.")
    );
    assert_eq!(report.response_source, "runtime-embedded-tool-loop");
    assert_eq!(report.lifecycle_phase_count, 8);
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that the mock provider can execute the bounded local tool loop.
fn execute_chat_turn_runs_mock_provider_tool_loop() {
    let mut bootstrap = test_bootstrap("mock-tool-loop");
    bootstrap.resolved_config.runtime_provider = Some("mock".to_string());
    bootstrap.resolved_config.runtime_model = Some("mock-1".to_string());
    std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        bootstrap
            .vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("need the tool loop".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Mock tool-informed final answer.")
    );
    assert_eq!(report.response_source, "runtime-mock-tool-loop");
    assert_eq!(report.lifecycle_phase_count, 8);
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that the mock provider can execute the bounded local tool loop during mixed text+image turns.
fn execute_chat_turn_runs_mock_provider_image_tool_loop() {
    let mut bootstrap = test_bootstrap("mock-image-tool-loop");
    bootstrap.resolved_config.runtime_provider = Some("mock".to_string());
    bootstrap.resolved_config.runtime_model = Some("mock-1".to_string());
    let image_path = bootstrap.vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();
    std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        bootstrap
            .vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("need the tool loop for this image".to_string()),
            image_present: true,
            image_path: Some(image_path.to_string_lossy().into_owned()),
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Mock tool-informed final answer.")
    );
    assert_eq!(report.response_source, "runtime-mock-tool-loop");
    assert_eq!(report.lifecycle_phase_count, 8);
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that the runtime can reflect on an invalid tool request and recover with a bounded retry.
fn execute_chat_turn_reflects_and_recovers_from_invalid_tool_request() {
    let (base_url, server) = spawn_mock_ollama_sequence(vec![
        MockOllamaExchange {
            response_body: r#"{"tool":"shell_exec"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "recover from invalid tool",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: "Recovered final answer.",
            expected_model: "gemma3:4b",
            prompt_fragment: "unsupported or malformed tool envelope",
            expected_image_base64: None,
        },
    ]);
    let mut bootstrap = test_bootstrap("ollama-reflect-recover");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("recover from invalid tool".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(report.response.as_deref(), Some("Recovered final answer."));
    assert_eq!(report.response_source, "runtime-ollama");
    assert_eq!(report.lifecycle_phase_count, 6);
    let inspection = inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("reflection inspection");
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| (record.phase.as_str(), record.step))
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            ("receive", None),
            ("deliberate", None),
            ("reflect", Some(1)),
            ("retry", Some(1)),
            ("respond", None),
            ("finish", None),
        ]
    );
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that the embedded provider can recover from one invalid tool request through the existing bounded reflection path.
fn execute_chat_turn_reflects_and_recovers_from_invalid_tool_request_with_embedded_provider() {
    let mut bootstrap = test_bootstrap("embedded-reflect-recover");
    bootstrap.resolved_config.runtime_provider = Some("embedded".to_string());
    let model_path = bootstrap.vela_home.join("models").join("gemma3.gguf");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"stub model").unwrap();
    bootstrap.resolved_config.runtime_embedded_model_path =
        Some(model_path.to_string_lossy().into_owned());

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("recover from invalid tool".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Embedded recovered answer.")
    );
    assert_eq!(report.response_source, "runtime-embedded");
    assert_eq!(report.lifecycle_phase_count, 6);
    let inspection = inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("embedded reflection inspection");
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| (record.phase.as_str(), record.step))
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            ("receive", None),
            ("deliberate", None),
            ("reflect", Some(1)),
            ("retry", Some(1)),
            ("respond", None),
            ("finish", None),
        ]
    );
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that mock image-backed turns can recover from one invalid provider tool request with bounded reflection.
fn execute_chat_turn_reflects_and_recovers_from_invalid_tool_request_during_mock_image_turn() {
    let mut bootstrap = test_bootstrap("mock-image-reflect-recover");
    bootstrap.resolved_config.runtime_provider = Some("mock".to_string());
    bootstrap.resolved_config.runtime_model = Some("mock-1".to_string());
    let image_path = bootstrap.vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("recover from invalid tool in this image".to_string()),
            image_present: true,
            image_path: Some(image_path.to_string_lossy().into_owned()),
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(report.response.as_deref(), Some("Mock recovered answer."));
    assert_eq!(report.response_source, "runtime-mock");
    assert_eq!(report.lifecycle_phase_count, 6);
    let inspection = inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("mock image reflection inspection");
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| (record.phase.as_str(), record.step))
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            ("receive", None),
            ("deliberate", None),
            ("reflect", Some(1)),
            ("retry", Some(1)),
            ("respond", None),
            ("finish", None),
        ]
    );
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that the iterative tool loop trips the max-step guard and falls back deterministically.
fn execute_chat_turn_stops_at_max_runtime_tool_steps() {
    let (base_url, server) = spawn_mock_ollama_sequence(vec![
        MockOllamaExchange {
            response_body: r#"{"tool":"memory_snapshot"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "trip the max-step guard",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"list_skills"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "Completed tool step 1 of 3.\nTool result for memory_snapshot:",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"memory_snapshot"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment:
                "Completed tool step 2 of 3.\nTool result for list_skills:\ndeploy-staging",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"list_skills"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "Completed tool step 3 of 3.\nTool result for memory_snapshot:",
            expected_image_base64: None,
        },
    ]);
    let mut bootstrap = test_bootstrap("ollama-tool-loop-max-step");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
    std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        bootstrap
            .vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("trip the max-step guard".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(report.response_source, "runtime-kernel");
    assert_eq!(report.lifecycle_phase_count, 10);
    assert_eq!(report.final_phase, "finish");
    assert!(report
        .response
        .as_deref()
        .unwrap_or_default()
        .contains("maximum bounded tool steps"));
    let inspection = inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("max-step inspection");
    assert_eq!(inspection.messages.len(), 8);
    assert_eq!(
        inspection
            .events
            .iter()
            .filter(|event| event.event_type == "runtime_tool_requested")
            .count(),
        3
    );
    assert_eq!(
        inspection
            .events
            .iter()
            .filter(|event| event.event_type == "runtime_tool_completed")
            .count(),
        3
    );
    let third_tool_result_metadata: serde_json::Value = serde_json::from_str(
        inspection.messages[6]
            .metadata_json
            .as_deref()
            .expect("third tool-result metadata"),
    )
    .expect("decode third tool-result metadata");
    assert_eq!(inspection.messages[6].role, "tool-result");
    assert_eq!(
        third_tool_result_metadata
            .get("request")
            .and_then(|v| v.get("tool"))
            .and_then(|v| v.as_str()),
        Some("memory_snapshot")
    );
    assert_eq!(
        third_tool_result_metadata
            .get("step")
            .and_then(|v| v.as_u64()),
        Some(3)
    );
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| (record.sequence, record.phase.as_str(), record.step))
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            (1, "receive", None),
            (2, "deliberate", None),
            (3, "tool-request", Some(1)),
            (4, "tool-result", Some(1)),
            (5, "tool-request", Some(2)),
            (6, "tool-result", Some(2)),
            (7, "tool-request", Some(3)),
            (8, "tool-result", Some(3)),
            (9, "respond", None),
            (10, "finish", None),
        ]
    );
    let assistant = inspection.messages.last().expect("assistant message");
    let assistant_metadata: serde_json::Value = serde_json::from_str(
        assistant
            .metadata_json
            .as_deref()
            .expect("assistant metadata"),
    )
    .expect("decode assistant metadata");
    assert_eq!(
        assistant_metadata.get("source").and_then(|v| v.as_str()),
        Some("runtime-kernel")
    );
    assert_eq!(
        assistant_metadata.get("provider").and_then(|v| v.as_str()),
        None
    );
    assert_eq!(
        assistant_metadata.get("model").and_then(|v| v.as_str()),
        None
    );
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that the runtime can recover when the post-tool-loop final provider continuation is invalid.
fn execute_chat_turn_recovers_from_invalid_final_provider_continuation_after_max_tool_steps() {
    let (base_url, server) = spawn_mock_ollama_sequence(vec![
        MockOllamaExchange {
            response_body: r#"{"tool":"memory_snapshot"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "recover after max-step invalid continuation",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"list_skills"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "Completed tool step 1 of 3.\nTool result for memory_snapshot:",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"memory_snapshot"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment:
                "Completed tool step 2 of 3.\nTool result for list_skills:\ndeploy-staging",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"shell_exec"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "Completed tool step 3 of 3.\nTool result for memory_snapshot:",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: "Recovered final answer after max-step reflection.",
            expected_model: "gemma3:4b",
            prompt_fragment: "You have already exhausted the maximum number of tool steps. Do not request another tool.",
            expected_image_base64: None,
        },
    ]);
    let mut bootstrap = test_bootstrap("ollama-final-invalid-recover");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
    std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        bootstrap
            .vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("recover after max-step invalid continuation".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Recovered final answer after max-step reflection.")
    );
    assert_eq!(report.response_source, "runtime-ollama-tool-loop");
    assert_eq!(report.lifecycle_phase_count, 12);
    let inspection = inspect_latest_session(&bootstrap, 24)
        .unwrap()
        .expect("final invalid reflection inspection");
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| (record.phase.as_str(), record.step))
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            ("receive", None),
            ("deliberate", None),
            ("tool-request", Some(1)),
            ("tool-result", Some(1)),
            ("tool-request", Some(2)),
            ("tool-result", Some(2)),
            ("tool-request", Some(3)),
            ("tool-result", Some(3)),
            ("reflect", Some(1)),
            ("retry", Some(1)),
            ("respond", None),
            ("finish", None),
        ]
    );
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that the runtime can recover when the post-tool-loop final provider continuation is empty.
fn execute_chat_turn_recovers_from_empty_final_provider_continuation_after_max_tool_steps() {
    let (base_url, server) = spawn_mock_ollama_sequence(vec![
        MockOllamaExchange {
            response_body: r#"{"tool":"memory_snapshot"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "recover after max-step empty continuation",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"list_skills"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "Completed tool step 1 of 3.\nTool result for memory_snapshot:",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"memory_snapshot"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment:
                "Completed tool step 2 of 3.\nTool result for list_skills:\ndeploy-staging",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: "   ",
            expected_model: "gemma3:4b",
            prompt_fragment: "Completed tool step 3 of 3.\nTool result for memory_snapshot:",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: "Recovered final answer after empty max-step reflection.",
            expected_model: "gemma3:4b",
            prompt_fragment: "You have already exhausted the maximum number of tool steps. Do not request another tool. Your previous reply was empty and unusable.",
            expected_image_base64: None,
        },
    ]);
    let mut bootstrap = test_bootstrap("ollama-final-empty-recover");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);
    std::fs::create_dir_all(bootstrap.vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        bootstrap
            .vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("recover after max-step empty continuation".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(
        report.response.as_deref(),
        Some("Recovered final answer after empty max-step reflection.")
    );
    assert_eq!(report.response_source, "runtime-ollama-tool-loop");
    assert_eq!(report.lifecycle_phase_count, 12);
    let inspection = inspect_latest_session(&bootstrap, 24)
        .unwrap()
        .expect("final empty reflection inspection");
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| (record.phase.as_str(), record.step))
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            ("receive", None),
            ("deliberate", None),
            ("tool-request", Some(1)),
            ("tool-result", Some(1)),
            ("tool-request", Some(2)),
            ("tool-result", Some(2)),
            ("tool-request", Some(3)),
            ("tool-result", Some(3)),
            ("reflect", Some(1)),
            ("retry", Some(1)),
            ("respond", None),
            ("finish", None),
        ]
    );
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that repeated invalid provider continuations fall back after the bounded reflection limit.
fn execute_chat_turn_falls_back_after_exhausting_reflection_retries() {
    let (base_url, server) = spawn_mock_ollama_sequence(vec![
        MockOllamaExchange {
            response_body: r#"{"tool":"shell_exec"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "exhaust reflection retries",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"shell_exec"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "unsupported or malformed tool envelope",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: r#"{"tool":"shell_exec"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "unsupported or malformed tool envelope",
            expected_image_base64: None,
        },
    ]);
    let mut bootstrap = test_bootstrap("ollama-reflect-fallback");
    bootstrap.resolved_config.runtime_provider = Some("ollama".to_string());
    bootstrap.resolved_config.runtime_model = Some("gemma3:4b".to_string());
    bootstrap.resolved_config.runtime_ollama_base_url = Some(base_url);

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("exhaust reflection retries".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(report.response_source, "runtime-kernel");
    assert!(report
        .response
        .as_deref()
        .unwrap_or_default()
        .contains("exhausted the bounded reflection limit"));
    assert_eq!(report.lifecycle_phase_count, 9);
    let inspection = inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("reflection fallback inspection");
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| (record.phase.as_str(), record.step))
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            ("receive", None),
            ("deliberate", None),
            ("reflect", Some(1)),
            ("retry", Some(1)),
            ("reflect", Some(2)),
            ("retry", Some(2)),
            ("reflect", Some(3)),
            ("respond", None),
            ("finish", None),
        ]
    );
    server.join().unwrap();
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that repeated invalid mock image-turn continuations fall back after the bounded reflection limit.
fn execute_chat_turn_falls_back_after_exhausting_reflection_retries_during_mock_image_turn() {
    let mut bootstrap = test_bootstrap("mock-image-reflect-fallback");
    bootstrap.resolved_config.runtime_provider = Some("mock".to_string());
    bootstrap.resolved_config.runtime_model = Some("mock-1".to_string());
    let image_path = bootstrap.vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();

    let report = execute_chat_turn(
        &bootstrap,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("exhaust reflection retries in this image".to_string()),
            image_present: true,
            image_path: Some(image_path.to_string_lossy().into_owned()),
            resume: None,
            continue_last: None,
        },
        None,
        None,
        false,
    )
    .unwrap();

    assert_eq!(report.response_source, "runtime-kernel");
    assert!(report
        .response
        .as_deref()
        .unwrap_or_default()
        .contains("exhausted the bounded reflection limit"));
    assert_eq!(report.lifecycle_phase_count, 9);
    let inspection = inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("mock image reflection fallback inspection");
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .filter(|record| record.turn_id == report.turn_id)
        .map(|record| (record.phase.as_str(), record.step))
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            ("receive", None),
            ("deliberate", None),
            ("reflect", Some(1)),
            ("retry", Some(1)),
            ("reflect", Some(2)),
            ("retry", Some(2)),
            ("reflect", Some(3)),
            ("respond", None),
            ("finish", None),
        ]
    );
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}
