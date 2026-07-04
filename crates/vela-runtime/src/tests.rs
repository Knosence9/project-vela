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
    assert_eq!(execution.provider_capabilities, Some(RuntimeProviderCapabilities {
        supports_text: true,
        supports_tool_loop: true,
        supports_reflection_retry: true,
        supports_images: true,
    }));
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
    assert_eq!(execution.provider_capabilities, Some(RuntimeProviderCapabilities {
        supports_text: true,
        supports_tool_loop: true,
        supports_reflection_retry: true,
        supports_images: false,
    }));
    assert_eq!(execution.model.as_deref(), Some("mock-1"));
    assert_eq!(provider.label(), "mock");
    assert_eq!(provider.model(), Some("mock-1"));
    assert_eq!(provider.direct_response_source(), "runtime-mock");
    assert_eq!(provider.tool_loop_response_source(), "runtime-mock-tool-loop");
    assert!(!provider.supports_images());
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
    assert!(drifted
        .summary_line()
        .contains("restart_required=runtime.provider@kernel-runtime,runtime.model@kernel-runtime,runtime.ollama_base_url@kernel-runtime ownership_blocked=true"));
    assert_eq!(
        drifted.ownership_block_reason().as_deref(),
        Some(
            "extension reload blocked by kernel-owned runtime drift: runtime.provider@kernel-runtime, runtime.model@kernel-runtime, runtime.ollama_base_url@kernel-runtime"
        )
    );
    assert!(drifted.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.provider"
            && item.owner == "kernel-runtime"
            && item.detail == "provider backend changes remain restart-only during extension reload"
    }));
    assert!(drifted.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.model"
            && item.owner == "kernel-runtime"
            && item.detail == "runtime model changes remain restart-only during extension reload"
    }));
    assert!(drifted.restart_required_drifts.iter().any(|item| {
        item.field == "runtime.ollama_base_url"
            && item.owner == "kernel-runtime"
            && item.detail
                == "provider transport endpoint changes remain restart-only during extension reload"
    }));

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

    let first = add_scheduled_job(&bootstrap, "0 * * * *", "ping status", None).unwrap();
    let jobs = list_scheduled_jobs(&bootstrap).unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, first.id);

    let err = add_scheduled_job(&bootstrap, "0 * * * *", "ping status", None).unwrap_err();
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
/// Verifies scheduler restart continuity while preserving registered durable jobs.
fn scheduler_start_resumes_same_session_and_preserves_registered_jobs() {
    let bootstrap = test_bootstrap("scheduler-resume");

    let first = start_scheduler(&bootstrap).unwrap();
    let first_summary = current_command_session_summary(&bootstrap, "cron")
        .unwrap()
        .expect("initial cron session summary");
    let added = add_scheduled_job(&bootstrap, "*/5 * * * *", "ping status", Some("test")).unwrap();
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
    let added = add_scheduled_job(&bootstrap, "* * * * *", "ping status", Some("test")).unwrap();
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
    std::fs::remove_dir_all(&bootstrap.vela_home).unwrap();
}

#[test]
/// Verifies that mock provider image requests fall back to the local kernel path when image support is unavailable.
fn execute_chat_turn_falls_back_for_mock_provider_image_requests() {
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

    assert_eq!(report.response_source, "runtime-kernel");
    assert!(report
        .response
        .as_deref()
        .unwrap_or_default()
        .contains("No provider-backed image execution was available"));
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

    assert_eq!(report.response.as_deref(), Some("Mock tool-informed final answer."));
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
