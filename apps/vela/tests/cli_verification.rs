use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Describes one expected mock Ollama request/response exchange.
struct MockOllamaExchange<'a> {
    response_body: &'a str,
    expected_model: &'a str,
    prompt_fragment: &'a str,
    expected_image_base64: Option<&'a str>,
}

/// Reads a full mock HTTP request body using the advertised content length.
fn read_mock_http_request(stream: &mut std::net::TcpStream) -> String {
    use std::io::Read;

    let mut request_bytes = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end;
    let expected_total_len;
    loop {
        let read = stream.read(&mut buf).expect("read mock ollama request");
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

/// Verifies that a captured mock Ollama request matches the expected exchange contract.
fn assert_mock_ollama_request(request: &str, exchange: &MockOllamaExchange<'_>) {
    let (head, body_text) = request
        .split_once("\r\n\r\n")
        .expect("split HTTP headers/body");
    let request_line = head.lines().next().expect("HTTP request line");
    assert!(
        request_line.starts_with("POST /api/generate HTTP/1.1"),
        "unexpected request line: {request_line}"
    );
    let payload: serde_json::Value =
        serde_json::from_str(body_text).expect("decode mock ollama JSON body");
    assert_eq!(
        payload.get("model").and_then(|v| v.as_str()),
        Some(exchange.expected_model)
    );
    assert_eq!(payload.get("stream").and_then(|v| v.as_bool()), Some(false));
    let prompt = payload
        .get("prompt")
        .and_then(|v| v.as_str())
        .expect("prompt field");
    assert!(
        prompt.contains(exchange.prompt_fragment),
        "prompt missing fragment: {}",
        exchange.prompt_fragment
    );
    let images = payload.get("images").and_then(|v| v.as_array());
    if let Some(expected_image_base64) = exchange.expected_image_base64 {
        let images = images.expect("images field");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].as_str(), Some(expected_image_base64));
    } else {
        assert!(
            payload.get("images").is_none(),
            "images field should be absent when no image is expected"
        );
    }
}

/// Spawns a mock Ollama server that validates one or more request/response exchanges.
fn spawn_mock_ollama_sequence(
    exchanges: Vec<MockOllamaExchange<'static>>,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock ollama");
    listener
        .set_nonblocking(true)
        .expect("configure mock ollama listener nonblocking mode");
    let addr = format!(
        "http://{}",
        listener.local_addr().expect("mock ollama addr")
    );
    let handle = std::thread::spawn(move || {
        for exchange in exchanges {
            let deadline = Instant::now() + Duration::from_secs(5);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(pair) => break pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for mock Ollama request"
                        );
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept mock ollama request: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set mock ollama read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .expect("set mock ollama write timeout");
            let request = read_mock_http_request(&mut stream);
            assert_mock_ollama_request(&request, &exchange);
            let payload = serde_json::json!({ "response": exchange.response_body }).to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            stream
                .write_all(reply.as_bytes())
                .expect("write mock ollama response");
            stream.flush().expect("flush mock ollama response");
        }
    });
    (addr, handle)
}

/// Spawns a one-shot mock Ollama server that validates the request contract.
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

/// Creates an isolated VELA_HOME path for one CLI integration test.
fn temp_vela_home(prefix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "vela-cli-{prefix}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

/// Runs the compiled `vela` binary against a temporary home with the given args.
fn run_vela(vela_home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vela"))
        .env("VELA_HOME", vela_home)
        .args(args)
        .output()
        .expect("run vela")
}

/// Decodes captured stdout for assertions.
fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Decodes captured stderr for assertions.
fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Extracts a `key=value` token from CLI output.
fn parse_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.split_whitespace()
        .find_map(|part| part.strip_prefix(&format!("{key}=")))
}

#[test]
/// Verifies that a default runtime session becomes visible through `vela status`.
fn default_runtime_session_surfaces_in_status() {
    let vela_home = temp_vela_home("status");

    let first = run_vela(&vela_home, &[]);
    assert!(first.status.success(), "{}", stderr_text(&first));
    let first_stdout = stdout_text(&first);
    assert!(first_stdout.contains("runtime session: action=created"));
    assert!(first_stdout.contains("Interactive Vela runtime ready."));

    let status = run_vela(&vela_home, &["status"]);
    assert!(status.status.success(), "{}", stderr_text(&status));
    let status_stdout = stdout_text(&status);
    assert!(status_stdout.contains("active session: id=session-"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that extension status surfaces activation-boundary outcomes for validated, activated, and failed entries.
fn extension_status_surfaces_activation_boundaries() {
    let vela_home = temp_vela_home("extension-activation-boundaries");
    std::fs::create_dir_all(vela_home.join("extensions")).unwrap();
    std::fs::write(
        vela_home.join("extensions").join("tool-meta.yaml"),
        "manifest_version: 1\nid: tool-meta\ntitle: Tool Meta\nkind: tool\nactivation: metadata-only\nentry: extensions/tool-meta.wasm\n",
    )
    .unwrap();
    std::fs::write(
        vela_home.join("extensions").join("workflow.yaml"),
        "manifest_version: 1\nid: workflow\ntitle: Workflow\nkind: workflow\nentry: extensions/workflow.flow\n",
    )
    .unwrap();
    std::fs::write(
        vela_home.join("extensions").join("service-on-boot.yaml"),
        "manifest_version: 1\nid: service-on-boot\ntitle: Service On Boot\nkind: service\nactivation: on-boot\n",
    )
    .unwrap();

    let status = run_vela(&vela_home, &["status"]);
    assert!(status.status.success(), "{}", stderr_text(&status));
    let status_stdout = stdout_text(&status);
    assert!(status_stdout.contains("validated=1"));
    assert!(status_stdout.contains("activated=1"));
    assert!(status_stdout.contains("failed=1"));
    assert!(status_stdout.contains("extension [validated]: id=Some(\"tool-meta\")"));
    assert!(status_stdout.contains("detail=Some(\"tool extension validated as metadata-only by manifest policy\")"));
    assert!(status_stdout.contains("extension [activated]: id=Some(\"workflow\")"));
    assert!(status_stdout.contains("detail=Some(\"workflow extension activated during bootstrap\")"));
    assert!(status_stdout.contains("extension [failed]: id=Some(\"service-on-boot\")"));
    assert!(status_stdout.contains("detail=Some(\"service extensions cannot request on-boot activation in this slice\")"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that extension status surfaces config-disabled entries and reload preserves the active session while surfacing restart-only runtime drift.
fn extensions_status_and_reload_are_visible_via_cli() {
    let vela_home = temp_vela_home("extensions");
    std::fs::create_dir_all(vela_home.join("extensions")).unwrap();
    std::fs::write(
        vela_home.join("extensions").join("demo.yaml"),
        "manifest_version: 1\nid: demo\ntitle: Demo\nkind: tool\nentry: extensions/demo-tool.wasm\ncapabilities:\n  - chat\n",
    )
    .unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: ollama\n  model: gemma3:4b\n  ollama_base_url: http://127.0.0.1:11434\nextensions:\n  entries:\n    demo:\n      enabled: false\n",
    )
    .unwrap();

    let status = run_vela(&vela_home, &["status"]);
    assert!(status.status.success(), "{}", stderr_text(&status));
    let status_stdout = stdout_text(&status);
    assert!(status_stdout.contains("extensions: dir="));
    assert!(status_stdout.contains("disabled=1"));
    assert!(status_stdout.contains("activation=Some(\"on-boot\")"));
    assert!(status_stdout.contains("extension [disabled]: id=Some(\"demo\")"));

    let session_turn = run_vela(&vela_home, &[]);
    assert!(
        session_turn.status.success(),
        "{}",
        stderr_text(&session_turn)
    );

    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: changed\n  ollama_base_url: http://127.0.0.1:22555\nextensions: {}\n",
    )
    .unwrap();
    let reload = run_vela(&vela_home, &["extensions", "--reload"]);
    assert!(reload.status.success(), "{}", stderr_text(&reload));
    let reload_stdout = stdout_text(&reload);
    assert!(reload_stdout.contains("extensions reloaded: extensions: dir="));
    assert!(reload_stdout.contains("activated=1"));
    assert!(reload_stdout.contains("session preserved: true"));
    assert!(reload_stdout.contains("restart required:"));
    assert!(reload_stdout.contains("extension [activated]: id=Some(\"demo\")"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that repeated gateway starts reuse the same command-scoped session.
fn gateway_start_resumes_same_session_via_cli() {
    let vela_home = temp_vela_home("gateway");

    let first = run_vela(&vela_home, &["gateway", "--start"]);
    assert!(first.status.success(), "{}", stderr_text(&first));
    let first_stdout = stdout_text(&first);
    let first_session = parse_field(&first_stdout, "session").expect("first gateway session id");

    let second = run_vela(&vela_home, &["gateway", "--start"]);
    assert!(second.status.success(), "{}", stderr_text(&second));
    let second_stdout = stdout_text(&second);
    let second_session = parse_field(&second_stdout, "session").expect("second gateway session id");
    assert_eq!(first_session, second_session);
    assert!(second_stdout.contains("action=resumed-latest"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a chat query executes a local runtime turn and can emit checkpoint artifacts.
fn chat_query_executes_runtime_turn_and_generates_candidates() {
    let vela_home = temp_vela_home("chat-turn");

    let turn = run_vela(
        &vela_home,
        &[
            "chat",
            "--query",
            "please always use terse answers",
            "--checkpoints",
        ],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Vela executed a local kernel turn."));
    assert!(turn_stdout.contains("lifecycle: turn=turn-"));
    assert!(turn_stdout.contains("phases=4"));
    assert!(turn_stdout.contains("last=finish"));
    assert!(turn_stdout.contains("checkpoints: signals=1 candidates=1"));

    let review = run_vela(&vela_home, &["review", "--list"]);
    assert!(review.status.success(), "{}", stderr_text(&review));
    let review_stdout = stdout_text(&review);
    assert!(review_stdout.contains("review candidates [1]:"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured Ollama provider is used for chat text turns.
fn chat_query_uses_configured_ollama_provider() {
    let vela_home = temp_vela_home("ollama-chat");
    let (base_url, server) =
        spawn_mock_ollama("Gemma local reply.", "gemma3:4b", "hello from cli", None);
    std::fs::create_dir_all(&vela_home).unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: ollama\n  model: gemma3:4b\n  ollama_base_url: {}\n",
            base_url
        ),
    )
    .unwrap();

    let turn = run_vela(&vela_home, &["chat", "--query", "hello from cli"]);
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Gemma local reply."));
    server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that an image-only chat turn still produces an assistant response.
fn chat_image_executes_runtime_turn() {
    let vela_home = temp_vela_home("image-turn");

    let turn = run_vela(&vela_home, &["chat", "--image", "diagram.png"]);
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Vela executed a local image turn."));
    assert!(turn_stdout.contains("Image: diagram.png"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured Ollama provider is used for chat image turns.
fn chat_image_uses_configured_ollama_provider() {
    let vela_home = temp_vela_home("ollama-image");
    let (base_url, server) = spawn_mock_ollama(
        "Gemma inspected the image.",
        "gemma3:4b",
        "Please analyze the attached image",
        Some("ZmFrZS1wbmctYnl0ZXM="),
    );
    std::fs::create_dir_all(&vela_home).unwrap();
    let image_path = vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: ollama\n  model: gemma3:4b\n  ollama_base_url: {}\n",
            base_url
        ),
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &["chat", "--image", image_path.to_str().expect("image path")],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Gemma inspected the image."));
    server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured provider turn can run a bounded multi-step local tool loop through the CLI.
fn chat_query_uses_configured_ollama_tool_loop() {
    let vela_home = temp_vela_home("ollama-tool-loop");
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
    std::fs::create_dir_all(vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: ollama\n  model: gemma3:4b\n  ollama_base_url: {}\n",
            base_url
        ),
    )
    .unwrap();

    let turn = run_vela(&vela_home, &["chat", "--query", "need the tool loop"]);
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Tool-informed final answer."));
    assert!(turn_stdout.contains("lifecycle: turn=turn-"));
    assert!(turn_stdout.contains("phases=8"));
    assert!(turn_stdout.contains("last=finish"));
    server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that the CLI runtime can recover from one invalid provider tool request with bounded reflection.
fn chat_query_recovers_from_invalid_tool_request() {
    let vela_home = temp_vela_home("ollama-reflect-recover");
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
    std::fs::create_dir_all(&vela_home).unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: ollama\n  model: gemma3:4b\n  ollama_base_url: {}\n",
            base_url
        ),
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &["chat", "--query", "recover from invalid tool"],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Recovered final answer."));
    assert!(turn_stdout.contains("lifecycle: turn=turn-"));
    assert!(turn_stdout.contains("phases=6"));
    assert!(turn_stdout.contains("last=finish"));

    std::env::set_var("VELA_HOME", &vela_home);
    let bootstrap = vela_runtime::initialize_bootstrap(None, false).unwrap();
    let inspection = vela_runtime::inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("cli reflection inspection");
    let lifecycle: Vec<_> = inspection
        .lifecycle
        .iter()
        .map(|record| record.phase.as_str())
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            "receive",
            "deliberate",
            "reflect",
            "retry",
            "respond",
            "finish"
        ]
    );
    std::env::remove_var("VELA_HOME");
    server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that the CLI runtime can retrieve targeted skill context through the provider tool loop.
fn chat_query_retrieves_targeted_skill_context() {
    let vela_home = temp_vela_home("ollama-skill-context");
    let (base_url, server) = spawn_mock_ollama_sequence(vec![
        MockOllamaExchange {
            response_body: r#"{"tool":"view_skill","name":"deploy-staging"}"#,
            expected_model: "gemma3:4b",
            prompt_fragment: "retrieve skill context",
            expected_image_base64: None,
        },
        MockOllamaExchange {
            response_body: "Skill-aware final answer.",
            expected_model: "gemma3:4b",
            prompt_fragment: "Tool result for view_skill:deploy-staging:\nskill deploy-staging:\n# deploy-staging\n\nDeploy staging safely.",
            expected_image_base64: None,
        },
    ]);
    std::fs::create_dir_all(vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploy staging safely.",
    )
    .unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: ollama\n  model: gemma3:4b\n  ollama_base_url: {}\n",
            base_url
        ),
    )
    .unwrap();

    let turn = run_vela(&vela_home, &["chat", "--query", "retrieve skill context"]);
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Skill-aware final answer."));
    server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies session branching, continue resolution, and compression through the CLI sessions surface.
fn sessions_branch_and_compress_are_inspectable() {
    let vela_home = temp_vela_home("sessions-branch");

    let first = run_vela(&vela_home, &["chat", "--query", "branch me"]);
    assert!(first.status.success(), "{}", stderr_text(&first));
    let first_stdout = stdout_text(&first);
    let parent_session = parse_field(&first_stdout, "id").expect("parent session id");
    let parent_title = parse_field(&first_stdout, "title").expect("parent session title");

    let branch = run_vela(
        &vela_home,
        &[
            "sessions",
            "--branch",
            parent_session,
            "--title",
            "branch-a",
            "--note",
            "explore alternative",
        ],
    );
    assert!(branch.status.success(), "{}", stderr_text(&branch));
    let branch_stdout = stdout_text(&branch);
    assert!(branch_stdout.contains("session branched:"));
    assert!(branch_stdout.contains("title=branch-a"));
    let branch_session = parse_field(&branch_stdout, "session").expect("branch session id");

    let branch_b = run_vela(
        &vela_home,
        &[
            "sessions",
            "--branch",
            parent_session,
            "--title",
            "branch-b",
        ],
    );
    assert!(branch_b.status.success(), "{}", stderr_text(&branch_b));
    let branch_b_stdout = stdout_text(&branch_b);
    let branch_b_session = parse_field(&branch_b_stdout, "session").expect("branch b session id");

    let branch_child = run_vela(
        &vela_home,
        &[
            "sessions",
            "--branch",
            branch_session,
            "--title",
            "branch-a-child",
        ],
    );
    assert!(
        branch_child.status.success(),
        "{}",
        stderr_text(&branch_child)
    );
    let branch_child_stdout = stdout_text(&branch_child);
    let branch_child_session =
        parse_field(&branch_child_stdout, "session").expect("branch child session id");

    let compress = run_vela(
        &vela_home,
        &[
            "sessions",
            "--compress",
            branch_session,
            "--summary",
            "branch compressed summary",
        ],
    );
    assert!(compress.status.success(), "{}", stderr_text(&compress));
    let compress_stdout = stdout_text(&compress);
    assert!(compress_stdout.contains("session compressed:"));

    let show = run_vela(&vela_home, &["sessions", "--show", branch_session]);
    assert!(show.status.success(), "{}", stderr_text(&show));
    let show_stdout = stdout_text(&show);
    assert!(show_stdout.contains("parent_id=Some"));
    assert!(show_stdout.contains("parent_title=Some"));
    assert!(show_stdout.contains("children [1]:"));
    assert!(show_stdout.contains("title=branch-a-child"));
    assert!(show_stdout.contains("compressions [1]:"));
    assert!(show_stdout.contains("summary=branch compressed summary"));

    let parent_show = run_vela(&vela_home, &["sessions", "--show", parent_session]);
    assert!(
        parent_show.status.success(),
        "{}",
        stderr_text(&parent_show)
    );
    let parent_show_stdout = stdout_text(&parent_show);
    assert!(parent_show_stdout.contains("children [2]:"));
    assert!(parent_show_stdout.contains("title=branch-a"));
    assert!(parent_show_stdout.contains("title=branch-b"));

    let continue_root = run_vela(
        &vela_home,
        &["chat", "--continue", parent_title, "--query", "follow root"],
    );
    assert!(
        continue_root.status.success(),
        "{}",
        stderr_text(&continue_root)
    );
    let continue_root_stdout = stdout_text(&continue_root);
    assert!(continue_root_stdout.contains(&format!("id={}", branch_child_session)));
    assert!(continue_root_stdout.contains("Session: branch-a-child"));
    assert!(continue_root_stdout.contains("continue resolution: mode=latest-in-subtree"));
    assert!(continue_root_stdout.contains(&format!("anchor_id=Some(\"{}\")", parent_session)));

    let continue_branch = run_vela(
        &vela_home,
        &["chat", "--continue", "branch-a", "--query", "follow branch"],
    );
    assert!(
        continue_branch.status.success(),
        "{}",
        stderr_text(&continue_branch)
    );
    let continue_branch_stdout = stdout_text(&continue_branch);
    assert!(continue_branch_stdout.contains(&format!("id={}", branch_child_session)));
    assert!(continue_branch_stdout.contains("Session: branch-a-child"));
    assert!(continue_branch_stdout.contains("continue resolution: mode=latest-in-subtree"));
    assert!(continue_branch_stdout.contains(&format!("anchor_id=Some(\"{}\")", branch_session)));

    let continue_exact = run_vela(
        &vela_home,
        &["chat", "--continue", "branch-b", "--query", "follow exact"],
    );
    assert!(continue_exact.status.success(), "{}", stderr_text(&continue_exact));
    let continue_exact_stdout = stdout_text(&continue_exact);
    assert!(continue_exact_stdout.contains(&format!("id={}", branch_b_session)));
    assert!(continue_exact_stdout.contains("continue resolution: mode=exact-anchor"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies cron job persistence and clap-level rejection of invalid flag combinations.
fn cron_registration_persists_and_invalid_flag_usage_is_rejected() {
    let vela_home = temp_vela_home("cron");

    let add = run_vela(
        &vela_home,
        &["cron", "--add", "ping status", "--schedule", "0 * * * *"],
    );
    assert!(add.status.success(), "{}", stderr_text(&add));
    let add_stdout = stdout_text(&add);
    let job_id = parse_field(&add_stdout, "added:")
        .or_else(|| parse_field(&add_stdout, "job"))
        .unwrap_or_else(|| add_stdout.split_whitespace().nth(3).expect("job id token"));

    let show = run_vela(&vela_home, &["cron", "--show", job_id]);
    assert!(show.status.success(), "{}", stderr_text(&show));
    let show_stdout = stdout_text(&show);
    assert!(show_stdout.contains(job_id));
    assert!(show_stdout.contains("task=ping status"));
    assert!(show_stdout.contains("next_run_at="));

    let list = run_vela(&vela_home, &["cron", "--list"]);
    assert!(list.status.success(), "{}", stderr_text(&list));
    let list_stdout = stdout_text(&list);
    assert!(list_stdout.contains(job_id));
    assert!(list_stdout.contains("run_count=0"));

    let invalid = run_vela(&vela_home, &["cron", "--schedule", "0 * * * *"]);
    assert!(!invalid.status.success());
    assert!(stderr_text(&invalid).contains("--add <ADD>"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that starting the scheduler executes due jobs and records durable run metadata.
fn cron_start_executes_due_jobs() {
    let vela_home = temp_vela_home("cron-start");

    let add = run_vela(
        &vela_home,
        &["cron", "--add", "ping status", "--schedule", "* * * * *"],
    );
    assert!(add.status.success(), "{}", stderr_text(&add));
    let add_stdout = stdout_text(&add);
    let job_id = parse_field(&add_stdout, "added:")
        .or_else(|| parse_field(&add_stdout, "job"))
        .unwrap_or_else(|| add_stdout.split_whitespace().nth(3).expect("job id token"));

    let jobs_path = vela_home.join("scheduler").join("jobs.json");
    let mut jobs: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&jobs_path).unwrap()).unwrap();
    jobs.as_array_mut().unwrap()[0]["next_run_at"] = serde_json::Value::from(1);
    std::fs::write(&jobs_path, serde_json::to_string_pretty(&jobs).unwrap()).unwrap();

    let start = run_vela(&vela_home, &["cron", "--start"]);
    assert!(start.status.success(), "{}", stderr_text(&start));
    let start_stdout = stdout_text(&start);
    assert!(start_stdout.contains("executed=1"));
    assert!(start_stdout.contains("recovered=0"));
    assert!(start_stdout.contains("failed=0"));

    let show = run_vela(&vela_home, &["cron", "--show", job_id]);
    assert!(show.status.success(), "{}", stderr_text(&show));
    let show_stdout = stdout_text(&show);
    assert!(show_stdout.contains("status=pending"));
    assert!(show_stdout.contains("run_count=1"));
    assert!(show_stdout.contains("outcome=Some(\"completed\")"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}
