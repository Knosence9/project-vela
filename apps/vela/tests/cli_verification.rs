use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Describes one expected mock Ollama request/response exchange.
struct MockOllamaExchange<'a> {
    response_body: &'a str,
    expected_model: &'a str,
    prompt_fragment: &'a str,
    expected_image_base64: Option<&'a str>,
}

/// Describes one expected mock llama.cpp request/response exchange.
struct MockLlamaCppExchange<'a> {
    expected_model: &'a str,
    prompt_fragment: &'a str,
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

/// Verifies that a captured mock llama.cpp request matches the expected exchange contract.
fn assert_mock_llamacpp_request(request: &str, exchange: &MockLlamaCppExchange<'_>) {
    let (head, body_text) = request
        .split_once("\r\n\r\n")
        .expect("split HTTP headers/body");
    let request_line = head.lines().next().expect("HTTP request line");
    assert!(
        request_line.starts_with("POST /completion HTTP/1.1"),
        "unexpected request line: {request_line}"
    );
    let payload: serde_json::Value =
        serde_json::from_str(body_text).expect("decode mock llama.cpp JSON body");
    assert_eq!(
        payload.get("model").and_then(|v| v.as_str()),
        Some(exchange.expected_model)
    );
    assert_eq!(payload.get("stream").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(payload.get("n_predict").and_then(|v| v.as_u64()), Some(256));
    let prompt = payload
        .get("prompt")
        .and_then(|v| v.as_str())
        .expect("prompt field");
    assert!(
        prompt.contains(exchange.prompt_fragment),
        "prompt missing fragment: {}",
        exchange.prompt_fragment
    );
}

/// Spawns a one-shot mock llama.cpp server that validates the request contract.
fn spawn_mock_llamacpp(
    response_body: &'static str,
    expected_model: &'static str,
    prompt_fragment: &'static str,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock llama.cpp");
    listener
        .set_nonblocking(true)
        .expect("configure mock llama.cpp listener nonblocking mode");
    let addr = format!(
        "http://{}",
        listener.local_addr().expect("mock llama.cpp addr")
    );
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(pair) => break pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for mock llama.cpp request"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept mock llama.cpp request: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set mock llama.cpp read timeout");
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .expect("set mock llama.cpp write timeout");
        let request = read_mock_http_request(&mut stream);
        assert_mock_llamacpp_request(
            &request,
            &MockLlamaCppExchange {
                expected_model,
                prompt_fragment,
            },
        );
        let payload = serde_json::json!({ "content": response_body }).to_string();
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream
            .write_all(reply.as_bytes())
            .expect("write mock llama.cpp response");
        stream.flush().expect("flush mock llama.cpp response");
    });
    (addr, handle)
}

/// Spawns a mock webhook endpoint that validates one JSON delivery request.
fn spawn_mock_webhook(
    expected_path: &'static str,
    expected_event_type: &'static str,
    expected_payload: &'static str,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock webhook");
    listener
        .set_nonblocking(true)
        .expect("configure mock webhook listener nonblocking mode");
    let addr = format!(
        "http://{}{}",
        listener.local_addr().expect("mock webhook addr"),
        expected_path
    );
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(pair) => break pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for mock webhook request"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept mock webhook request: {error}"),
            }
        };
        let request = read_mock_http_request(&mut stream);
        let (head, body_text) = request
            .split_once("\r\n\r\n")
            .expect("split HTTP headers/body");
        let request_line = head.lines().next().expect("HTTP request line");
        assert!(
            request_line.starts_with(&format!("POST {expected_path} HTTP/1.1")),
            "unexpected request line: {request_line}"
        );
        let payload: serde_json::Value =
            serde_json::from_str(body_text).expect("decode mock webhook JSON body");
        assert_eq!(
            payload.get("event_type").and_then(|v| v.as_str()),
            Some(expected_event_type)
        );
        assert_eq!(
            payload.get("payload").and_then(|v| v.as_str()),
            Some(expected_payload)
        );
        assert_eq!(
            payload.get("source").and_then(|v| v.as_str()),
            Some("gateway")
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .expect("write mock webhook response");
    });
    (addr, handle)
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

static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

struct ScopedEnvVar {
    key: &'static str,
    previous: Option<OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl ScopedEnvVar {
    fn set(key: &'static str, value: impl Into<OsString>) -> Self {
        let guard = ENV_MUTEX
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("lock env mutex");
        let previous = std::env::var_os(key);
        std::env::set_var(key, value.into());
        Self {
            key,
            previous,
            _guard: guard,
        }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
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

fn parse_list_item_id<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(prefix)
            .and_then(|rest| rest.split_whitespace().next())
    })
}

#[test]
/// Verifies that a default runtime session becomes visible through `vela status`.
fn default_runtime_session_surfaces_in_status() {
    let vela_home = temp_vela_home("status");

    let first = run_vela(&vela_home, &[]);
    assert!(first.status.success(), "{}", stderr_text(&first));
    let first_stdout = stdout_text(&first);
    assert!(first_stdout.contains("runtime session: action=created state=finish"));
    assert!(first_stdout.contains("title=chat interactive"));
    assert!(first_stdout.contains("Interactive Vela runtime ready."));

    let status = run_vela(&vela_home, &["status"]);
    assert!(status.status.success(), "{}", stderr_text(&status));
    let status_stdout = stdout_text(&status);
    assert!(status_stdout.contains("backend api [4]:"));
    assert!(status_stdout.contains("id=ollama transport=http-json"));
    assert!(status_stdout.contains("id=mock transport=in-process"));
    assert!(status_stdout.contains("id=llamacpp transport=http-json"));
    assert!(status_stdout.contains("id=embedded transport=in-process"));
    assert!(status_stdout.contains("resolved backend: none"));
    assert!(status_stdout.contains("resolved backend readiness: none"));
    assert!(status_stdout.contains("active session: id=session-"));
    assert!(status_stdout.contains("title=chat interactive"));
    assert!(status_stdout.contains("state=finish"));

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
    assert!(status_stdout
        .contains("detail=Some(\"tool extension validated as metadata-only by manifest policy\")"));
    assert!(status_stdout.contains("extension [activated]: id=Some(\"workflow\")"));
    assert!(
        status_stdout.contains("detail=Some(\"workflow extension activated during bootstrap\")")
    );
    assert!(status_stdout.contains("extension [failed]: id=Some(\"service-on-boot\")"));
    assert!(status_stdout.contains(
        "detail=Some(\"service extensions cannot request on-boot activation in this slice\")"
    ));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that the embedded backend contract and config plumbing are visible through `vela status`.
fn embedded_backend_contract_and_config_are_visible_via_status() {
    let vela_home = temp_vela_home("embedded-status");
    let model_path = vela_home.join("models").join("gemma3.gguf");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"stub model").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: embedded\n  embedded_model_path: {}\n",
            model_path.display()
        ),
    )
    .unwrap();

    let status = run_vela(&vela_home, &["status"]);
    assert!(status.status.success(), "{}", stderr_text(&status));
    let status_stdout = stdout_text(&status);
    assert!(status_stdout.contains("runtime.provider=Some(\"embedded\")"));
    assert!(status_stdout.contains("runtime.embedded_model_path=Some("));
    assert!(status_stdout.contains("id=embedded transport=in-process"));
    assert!(status_stdout.contains("resolved backend: api=v1 id=embedded transport=in-process"));
    assert!(status_stdout.contains("resolved backend readiness: ok"));
    assert!(status_stdout.contains("embedded lifecycle: state=fixture-ready"));
    assert!(status_stdout.contains("fixture_shims=true"));
    assert!(status_stdout.contains("restart_on_model_change=true"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that embedded status rejects non-GGUF model paths before execution and surfaces the guardrail state.
fn embedded_status_rejects_non_gguf_model_paths() {
    let vela_home = temp_vela_home("embedded-invalid-ext");
    let model_path = vela_home.join("models").join("gemma3.txt");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"not a gguf").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: embedded\n  embedded_model_path: {}\n",
            model_path.display()
        ),
    )
    .unwrap();

    let status = run_vela(&vela_home, &["status"]);
    assert!(status.status.success(), "{}", stderr_text(&status));
    let status_stdout = stdout_text(&status);
    assert!(status_stdout.contains("resolved backend readiness: error (runtime provider 'embedded' requires runtime.embedded_model_path to point to a .gguf model file)"));
    assert!(status_stdout.contains("embedded lifecycle: state=invalid-config"));
    assert!(status_stdout.contains("expected=.gguf"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that embedded status rejects empty GGUF model files before execution and surfaces the guardrail state.
fn embedded_status_rejects_empty_gguf_model_paths() {
    let vela_home = temp_vela_home("embedded-empty-gguf");
    let model_path = vela_home.join("models").join("empty.gguf");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: embedded\n  embedded_model_path: {}\n",
            model_path.display()
        ),
    )
    .unwrap();

    let status = run_vela(&vela_home, &["status"]);
    assert!(status.status.success(), "{}", stderr_text(&status));
    let status_stdout = stdout_text(&status);
    assert!(status_stdout.contains("resolved backend readiness: error (runtime provider 'embedded' requires runtime.embedded_model_path to point to a non-empty .gguf model file)"));
    assert!(status_stdout.contains("embedded lifecycle: state=invalid-config"));
    assert!(status_stdout.contains("file_size_bytes=0"));
    assert!(status_stdout.contains("expected=non-empty"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that embedded load failures persist a visible lifecycle state for later status inspection.
fn embedded_status_surfaces_last_load_failure() {
    let vela_home = temp_vela_home("embedded-load-failed");
    let model_path = vela_home.join("models").join("broken.gguf");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"not a real gguf").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: embedded\n  embedded_model_path: {}\n",
            model_path.display()
        ),
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &["chat", "--query", "hello from broken embedded"],
    );
    assert!(!turn.status.success());
    let turn_stderr = stderr_text(&turn);
    assert!(turn_stderr.contains("failed to load embedded model from"));

    let status = run_vela(&vela_home, &["status"]);
    assert!(status.status.success(), "{}", stderr_text(&status));
    let status_stdout = stdout_text(&status);
    assert!(status_stdout.contains("resolved backend readiness: ok"));
    assert!(status_stdout.contains("embedded lifecycle: state=load-failed"));
    assert!(status_stdout.contains("last_error=failed to load embedded model from"));
    assert!(status_stdout.contains("state_file="));

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
    assert!(status_stdout.contains(&format!(
        "runtime ownership: path={} source=durable-baseline status=aligned restart_required=none",
        vela_home
            .join("runtime")
            .join("reload-ownership-baseline.json")
            .display()
    )));
    assert!(status_stdout.contains("runtime ownership baseline: path="));
    assert!(status_stdout.contains("values=display.interface=null"));
    assert!(status_stdout.contains("runtime.provider=\"ollama\""));
    assert!(status_stdout.contains("runtime.model=\"gemma3:4b\""));
    assert!(status_stdout.contains("runtime.ollama_base_url=\"http://127.0.0.1:11434\""));
    assert!(status_stdout.contains("runtime ownership drifts: none"));

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
    let status_after_change = run_vela(&vela_home, &["status"]);
    assert!(
        status_after_change.status.success(),
        "{}",
        stderr_text(&status_after_change)
    );
    let status_after_change_stdout = stdout_text(&status_after_change);
    assert!(status_after_change_stdout.contains(&format!(
        "runtime ownership: path={} source=durable-baseline status=restart-required restart_required=runtime.provider@kernel-runtime,runtime.model@kernel-runtime,runtime.ollama_base_url@kernel-runtime",
        vela_home
            .join("runtime")
            .join("reload-ownership-baseline.json")
            .display()
    )));
    assert!(status_after_change_stdout.contains("runtime ownership baseline: path="));
    assert!(status_after_change_stdout.contains("runtime.provider=\"ollama\""));
    assert!(status_after_change_stdout.contains("runtime.model=\"gemma3:4b\""));
    assert!(status_after_change_stdout.contains(
        "runtime ownership [runtime.provider]: owner=kernel-runtime detail=provider backend changes remain restart-only during extension reload previous=\"ollama\" current=\"mock\" action=restart-required"
    ));
    assert!(status_after_change_stdout.contains(
        "runtime ownership [runtime.model]: owner=kernel-runtime detail=runtime model changes remain restart-only during extension reload previous=\"gemma3:4b\" current=\"changed\" action=restart-required"
    ));
    assert!(status_after_change_stdout.contains(
        "runtime ownership [runtime.ollama_base_url]: owner=kernel-runtime detail=provider transport endpoint changes remain restart-only during extension reload previous=\"http://127.0.0.1:11434\" current=\"http://127.0.0.1:22555\" action=restart-required"
    ));
    let reload = run_vela(&vela_home, &["extensions", "--reload"]);
    assert!(!reload.status.success());
    let reload_stdout = stdout_text(&reload);
    let reload_stderr = stderr_text(&reload);
    assert!(reload_stdout.contains("extensions reloaded: extensions: dir="));
    assert!(reload_stdout.contains("activated=1"));
    assert!(reload_stdout.contains("session preserved: true"));
    assert!(reload_stdout.contains(&format!(
        "ownership baseline: path={} source=durable-baseline",
        vela_home
            .join("runtime")
            .join("reload-ownership-baseline.json")
            .display()
    )));
    assert!(reload_stdout.contains("values=display.interface=null"));
    assert!(reload_stdout.contains("runtime.provider=\"ollama\""));
    assert!(reload_stdout.contains("runtime.model=\"gemma3:4b\""));
    assert!(reload_stdout.contains(
        "restart required: runtime.provider@kernel-runtime, runtime.model@kernel-runtime, runtime.ollama_base_url@kernel-runtime"
    ));
    assert!(reload_stdout.contains(
        "restart required [runtime.provider]: owner=kernel-runtime detail=provider backend changes remain restart-only during extension reload previous=\"ollama\" reloaded=\"mock\" action=restart-required"
    ));
    assert!(reload_stdout.contains(
        "restart required [runtime.model]: owner=kernel-runtime detail=runtime model changes remain restart-only during extension reload previous=\"gemma3:4b\" reloaded=\"changed\" action=restart-required"
    ));
    assert!(reload_stdout.contains(
        "restart required [runtime.ollama_base_url]: owner=kernel-runtime detail=provider transport endpoint changes remain restart-only during extension reload previous=\"http://127.0.0.1:11434\" reloaded=\"http://127.0.0.1:22555\" action=restart-required"
    ));
    assert!(reload_stdout.contains("extension [activated]: id=Some(\"demo\")"));
    assert!(reload_stderr.contains(&format!(
        "extension reload blocked by kernel-owned runtime drift: runtime.provider@kernel-runtime, runtime.model@kernel-runtime, runtime.ollama_base_url@kernel-runtime (restart vela with the updated config to refresh the ownership baseline at {})",
        vela_home
            .join("runtime")
            .join("reload-ownership-baseline.json")
            .display()
    )));

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
/// Verifies that bounded subagent delegation is exposed through the CLI and remains inspectable.
fn agents_delegation_is_visible_via_cli() {
    let vela_home = temp_vela_home("agents");

    let delegate = run_vela(
        &vela_home,
        &[
            "agents",
            "--delegate",
            "Investigate provider routing",
            "--role",
            "researcher",
            "--note",
            "bounded follow-up",
        ],
    );
    assert!(delegate.status.success(), "{}", stderr_text(&delegate));
    let delegate_stdout = stdout_text(&delegate);
    assert!(delegate_stdout.contains("delegation requested: id=delegation-"));
    assert!(delegate_stdout.contains("role=researcher"));
    assert!(delegate_stdout.contains("status=pending"));

    let listing = run_vela(&vela_home, &["agents", "--list"]);
    assert!(listing.status.success(), "{}", stderr_text(&listing));
    let listing_stdout = stdout_text(&listing);
    assert!(listing_stdout.contains("delegations [1]:"));
    assert!(listing_stdout.contains("role=researcher"));
    assert!(listing_stdout.contains("Investigate provider routing"));

    let duplicate = run_vela(
        &vela_home,
        &[
            "agents",
            "--delegate",
            "Investigate provider routing",
            "--role",
            "researcher",
        ],
    );
    assert!(!duplicate.status.success());
    assert!(stderr_text(&duplicate).contains("already pending"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that bounded MCP bridge requests are exposed through the CLI and remain inspectable.
fn mcp_bridge_requests_are_visible_via_cli() {
    let vela_home = temp_vela_home("mcp");

    let bridge = run_vela(
        &vela_home,
        &[
            "mcp",
            "--bridge",
            "memory",
            "--tool",
            "list_tools",
            "--payload",
            "{}",
            "--note",
            "bounded bridge request",
        ],
    );
    assert!(bridge.status.success(), "{}", stderr_text(&bridge));
    let bridge_stdout = stdout_text(&bridge);
    assert!(bridge_stdout.contains("mcp bridge requested: id=mcp-bridge-"));
    assert!(bridge_stdout.contains("server=memory"));
    assert!(bridge_stdout.contains("tool=list_tools"));
    assert!(bridge_stdout.contains("status=pending"));

    let listing = run_vela(&vela_home, &["mcp", "--list"]);
    assert!(listing.status.success(), "{}", stderr_text(&listing));
    let listing_stdout = stdout_text(&listing);
    assert!(listing_stdout.contains("mcp bridge requests [1]:"));
    assert!(listing_stdout.contains("server=memory"));
    assert!(listing_stdout.contains("tool=list_tools"));

    let invalid = run_vela(
        &vela_home,
        &[
            "mcp",
            "--bridge",
            "memory",
            "--tool",
            "list_tools",
            "--payload",
            "not-json",
        ],
    );
    assert!(!invalid.status.success());
    assert!(stderr_text(&invalid).contains("must be valid JSON"));

    let duplicate = run_vela(
        &vela_home,
        &[
            "mcp",
            "--bridge",
            "memory",
            "--tool",
            "list_tools",
            "--payload",
            "{}",
        ],
    );
    assert!(!duplicate.status.success());
    assert!(stderr_text(&duplicate).contains("already pending"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that gateway webhook delivery is exposed as a real external CLI surface.
fn gateway_webhook_delivery_executes_via_cli() {
    let vela_home = temp_vela_home("gateway-webhook");
    let (url, server) = spawn_mock_webhook("/deliver", "delivery.test", "hello webhook");

    let delivery = run_vela(
        &vela_home,
        &[
            "gateway",
            "--webhook-url",
            &url,
            "--payload",
            "hello webhook",
            "--event-type",
            "delivery.test",
        ],
    );
    assert!(delivery.status.success(), "{}", stderr_text(&delivery));
    let delivery_stdout = stdout_text(&delivery);
    assert!(delivery_stdout.contains("gateway webhook delivered: session="));
    assert!(delivery_stdout.contains("status=200"));
    assert!(delivery_stdout.contains("event=delivery.test"));

    server.join().unwrap();
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
    assert!(turn_stdout.contains("runtime session: action=created state=finish"));
    assert!(turn_stdout.contains("title=chat: please always use terse answers"));
    assert!(turn_stdout.contains("Vela executed a local kernel turn."));
    assert!(turn_stdout.contains("lifecycle: turn=turn-"));
    assert!(turn_stdout.contains("phases=4"));
    assert!(turn_stdout.contains("last=finish"));
    assert!(turn_stdout.contains("checkpoints: signals=1 candidates=1"));

    let show = run_vela(
        &vela_home,
        &[
            "sessions",
            "--show",
            "chat: please always use terse answers",
        ],
    );
    assert!(show.status.success(), "{}", stderr_text(&show));
    let show_stdout = stdout_text(&show);
    assert!(show_stdout.contains("session inspect: id=session-"));
    assert!(show_stdout.contains("title=chat: please always use terse answers"));
    assert!(show_stdout.contains("state=finish"));

    let review = run_vela(&vela_home, &["review", "--list"]);
    assert!(review.status.success(), "{}", stderr_text(&review));
    let review_stdout = stdout_text(&review);
    assert!(review_stdout.contains("review candidates [1]:"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a transcript-derived review candidate can be promoted into pending memory and approved.
fn review_candidate_can_be_promoted_and_approved_via_cli() {
    let vela_home = temp_vela_home("review-promote-approve");

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

    let review = run_vela(&vela_home, &["review", "--list"]);
    assert!(review.status.success(), "{}", stderr_text(&review));
    let review_stdout = stdout_text(&review);
    let candidate_id = parse_list_item_id(&review_stdout, "- ").expect("review candidate id");

    let show_candidate = run_vela(&vela_home, &["review", "--show", candidate_id]);
    assert!(
        show_candidate.status.success(),
        "{}",
        stderr_text(&show_candidate)
    );
    let show_candidate_stdout = stdout_text(&show_candidate);
    let show_candidate_json: serde_json::Value =
        serde_json::from_str(&show_candidate_stdout).expect("review --show JSON");
    assert_eq!(
        show_candidate_json
            .get("source")
            .and_then(|value| value.as_str()),
        Some("session-transcript")
    );
    assert_eq!(
        show_candidate_json
            .get("memory")
            .and_then(|value| value.get("action"))
            .and_then(|value| value.as_str()),
        Some("add")
    );
    assert!(show_candidate_stdout.contains("please always use terse answers"));

    let promote = run_vela(&vela_home, &["review", "--promote", candidate_id]);
    assert!(promote.status.success(), "{}", stderr_text(&promote));
    let promote_stdout = stdout_text(&promote);
    assert!(promote_stdout.contains("review promoted: candidate="));
    assert!(promote_stdout.contains("kind=memory"));
    let pending_id = parse_field(&promote_stdout, "pending").expect("pending memory id");

    let pending = run_vela(&vela_home, &["memory", "--pending"]);
    assert!(pending.status.success(), "{}", stderr_text(&pending));
    let pending_stdout = stdout_text(&pending);
    assert!(pending_stdout.contains("pending memory writes [1]:"));
    assert!(pending_stdout.contains(pending_id));

    let show_pending = run_vela(&vela_home, &["memory", "--show", pending_id]);
    assert!(
        show_pending.status.success(),
        "{}",
        stderr_text(&show_pending)
    );
    let show_pending_stdout = stdout_text(&show_pending);
    let show_pending_json: serde_json::Value =
        serde_json::from_str(&show_pending_stdout).expect("memory --show JSON");
    assert_eq!(
        show_pending_json
            .get("action")
            .and_then(|value| value.as_str()),
        Some("add")
    );
    assert_eq!(
        show_pending_json
            .get("target")
            .and_then(|value| value.as_str()),
        Some("User")
    );
    assert!(show_pending_json
        .get("new_text")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value.contains("please always use terse answers")));

    let approve = run_vela(&vela_home, &["memory", "--approve", pending_id]);
    assert!(approve.status.success(), "{}", stderr_text(&approve));
    let approve_stdout = stdout_text(&approve);
    assert!(approve_stdout.contains("memory approve: target=user entries=1"));

    let memory_view = run_vela(&vela_home, &["memory", "--target", "user"]);
    assert!(
        memory_view.status.success(),
        "{}",
        stderr_text(&memory_view)
    );
    let memory_view_stdout = stdout_text(&memory_view);
    assert!(memory_view_stdout.contains("user [1 entries,"));
    assert!(memory_view_stdout.contains("please always use terse answers"));

    let pending_after = run_vela(&vela_home, &["memory", "--pending"]);
    assert!(
        pending_after.status.success(),
        "{}",
        stderr_text(&pending_after)
    );
    assert!(stdout_text(&pending_after).contains("pending memory writes [0]:"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a transcript-derived review candidate can be promoted into pending memory and rejected.
fn review_candidate_can_be_promoted_and_rejected_via_cli() {
    let vela_home = temp_vela_home("review-promote-reject");

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

    let review = run_vela(&vela_home, &["review", "--list"]);
    assert!(review.status.success(), "{}", stderr_text(&review));
    let review_stdout = stdout_text(&review);
    let candidate_id = parse_list_item_id(&review_stdout, "- ").expect("review candidate id");

    let promote = run_vela(&vela_home, &["review", "--promote", candidate_id]);
    assert!(promote.status.success(), "{}", stderr_text(&promote));
    let promote_stdout = stdout_text(&promote);
    let pending_id = parse_field(&promote_stdout, "pending").expect("pending memory id");

    let reject = run_vela(&vela_home, &["memory", "--reject", pending_id]);
    assert!(reject.status.success(), "{}", stderr_text(&reject));
    assert!(stdout_text(&reject).contains(&format!("memory reject: {pending_id}")));

    let pending_after = run_vela(&vela_home, &["memory", "--pending"]);
    assert!(
        pending_after.status.success(),
        "{}",
        stderr_text(&pending_after)
    );
    assert!(stdout_text(&pending_after).contains("pending memory writes [0]:"));

    let memory_view = run_vela(&vela_home, &["memory", "--target", "user"]);
    assert!(
        memory_view.status.success(),
        "{}",
        stderr_text(&memory_view)
    );
    assert!(stdout_text(&memory_view).contains("user [0 entries,"));

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
    assert!(turn_stdout.contains("response route: source=runtime-ollama provider=ollama model=gemma3:4b capabilities=text=true tool_loop=true reflection_retry=true images=true"));
    server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured llama.cpp provider is used for chat text turns.
fn chat_query_uses_configured_llamacpp_provider() {
    let vela_home = temp_vela_home("llamacpp-chat");
    let (base_url, server) =
        spawn_mock_llamacpp("Phi local reply.", "phi-3-mini", "hello from llama.cpp cli");
    std::fs::create_dir_all(&vela_home).unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: llamacpp\n  model: phi-3-mini\n  llamacpp_base_url: {}\n",
            base_url
        ),
    )
    .unwrap();

    let turn = run_vela(&vela_home, &["chat", "--query", "hello from llama.cpp cli"]);
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Phi local reply."));
    assert!(turn_stdout.contains("response route: source=runtime-llamacpp provider=llamacpp model=phi-3-mini capabilities=text=true tool_loop=true reflection_retry=true images=false"));
    server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that the backend eval harness can compare bounded backends and persist the run for inspection.
fn backend_eval_harness_compares_backends_and_persists_results() {
    let vela_home = temp_vela_home("eval-harness");
    let (llamacpp_base_url, llamacpp_server) =
        spawn_mock_llamacpp("Phi eval reply.", "phi-3-mini", "compare backend behavior");
    std::fs::create_dir_all(&vela_home).unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  model: phi-3-mini\n  llamacpp_base_url: {}\n",
            llamacpp_base_url
        ),
    )
    .unwrap();

    let run = run_vela(
        &vela_home,
        &[
            "eval",
            "--run",
            "compare backend behavior",
            "--backend",
            "mock",
            "--backend",
            "llamacpp",
            "--model",
            "phi-3-mini",
        ],
    );
    assert!(run.status.success(), "{}", stderr_text(&run));
    let run_stdout = stdout_text(&run);
    assert!(run_stdout.contains("backend eval run: id=eval-"));
    assert!(run_stdout.contains("slot=None"));
    assert!(run_stdout.contains("parity_summary=Some(\""));
    assert!(run_stdout.contains("parity=diverged"));
    assert!(run_stdout.contains("passed=mock,llamacpp"));
    assert!(run_stdout.contains("capability_groups="));
    assert!(run_stdout.contains("backend=mock transport=in-process status=passed"));
    assert!(run_stdout.contains("backend=llamacpp transport=http-json status=passed"));
    let eval_id = parse_field(&run_stdout, "id").expect("eval id").to_string();

    let list = run_vela(&vela_home, &["eval", "--list"]);
    assert!(list.status.success(), "{}", stderr_text(&list));
    let list_stdout = stdout_text(&list);
    assert!(list_stdout.contains("backend eval runs [1]:"));
    assert!(list_stdout.contains(&eval_id));
    assert!(list_stdout.contains("slot=None"));
    assert!(list_stdout.contains("backends=mock,llamacpp"));
    assert!(list_stdout.contains("parity_summary=Some(\""));
    assert!(list_stdout.contains("parity=diverged"));
    assert!(list_stdout.contains("passed=mock,llamacpp"));
    assert!(list_stdout.contains("capability_groups="));

    let show = run_vela(&vela_home, &["eval", "--show", &eval_id]);
    assert!(show.status.success(), "{}", stderr_text(&show));
    let show_stdout = stdout_text(&show);
    assert!(show_stdout.contains(&format!("backend eval: id={}", eval_id)));
    assert!(show_stdout.contains("slot=None"));
    assert!(show_stdout.contains("parity_summary=Some(\""));
    assert!(show_stdout.contains("parity=diverged"));
    assert!(show_stdout.contains("passed=mock,llamacpp"));
    assert!(show_stdout.contains("capability_groups="));
    assert!(show_stdout.contains("backend=mock transport=in-process status=passed"));
    assert!(show_stdout.contains("backend=llamacpp transport=http-json status=passed"));
    llamacpp_server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that the backend eval harness can record embedded results alongside other backends through the durable eval surface.
fn backend_eval_harness_records_embedded_results() {
    let vela_home = temp_vela_home("eval-harness-embedded");
    let model_path = vela_home.join("models").join("gemma3.gguf");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"stub model").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: embedded\n  embedded_model_path: {}\n",
            model_path.display()
        ),
    )
    .unwrap();

    let run = run_vela(
        &vela_home,
        &[
            "eval",
            "--run",
            "compare embedded eval evidence",
            "--backend",
            "embedded",
            "--backend",
            "mock",
        ],
    );
    assert!(run.status.success(), "{}", stderr_text(&run));
    let run_stdout = stdout_text(&run);
    assert!(run_stdout.contains("backend eval run: id=eval-"));
    assert!(run_stdout.contains("slot=None"));
    assert!(run_stdout.contains("backends=embedded,mock"));
    assert!(run_stdout.contains("parity_summary=Some(\""));
    assert!(run_stdout.contains("parity=diverged"));
    assert!(run_stdout.contains("passed=embedded,mock"));
    assert!(run_stdout.contains("backend=embedded transport=in-process status=passed"));
    assert!(run_stdout.contains("source=Some(\"runtime-embedded\")"));
    assert!(run_stdout.contains("backend=mock transport=in-process status=passed"));
    let eval_id = parse_field(&run_stdout, "id").expect("eval id").to_string();

    let show = run_vela(&vela_home, &["eval", "--show", &eval_id]);
    assert!(show.status.success(), "{}", stderr_text(&show));
    let show_stdout = stdout_text(&show);
    assert!(show_stdout.contains(&format!("backend eval: id={}", eval_id)));
    assert!(show_stdout.contains("backends=embedded,mock"));
    assert!(show_stdout.contains("backend=embedded transport=in-process status=passed"));
    assert!(show_stdout.contains("backend=mock transport=in-process status=passed"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that the model-lab policy is visible through the eval surface.
fn model_lab_policy_is_visible_via_eval_surface() {
    let vela_home = temp_vela_home("model-lab-policy");

    let show_policy = run_vela(&vela_home, &["eval", "--show-policy"]);
    assert!(
        show_policy.status.success(),
        "{}",
        stderr_text(&show_policy)
    );
    let policy_stdout = stdout_text(&show_policy);
    assert!(policy_stdout.contains("model lab policy: version=1"));
    assert!(policy_stdout.contains(
        "allowed strategies [3]: shadow-routing,offline replay,bounded backend comparison"
    ));
    assert!(policy_stdout.contains("graduation gates [3]:"));
    assert!(policy_stdout.contains("required evidence [3]:"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that the bounded provider experiment slots are published and can drive persisted eval runs.
fn backend_experiment_slot_is_visible_and_runnable() {
    let vela_home = temp_vela_home("eval-slot");
    std::fs::create_dir_all(&vela_home).unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: mock-1\n",
    )
    .unwrap();

    let list_slots = run_vela(&vela_home, &["eval", "--list-slots"]);
    assert!(list_slots.status.success(), "{}", stderr_text(&list_slots));
    let list_slots_stdout = stdout_text(&list_slots);
    assert!(list_slots_stdout.contains("backend experiment slots [5]:"));
    assert!(list_slots_stdout
        .contains("ternary-preview :: status=bounded-preview strategy=shadow-routing"));
    assert!(list_slots_stdout
        .contains("sparse-routing-preview :: status=bounded-preview strategy=shadow-routing"));
    assert!(list_slots_stdout.contains("latest_eval_id=None"));
    assert!(list_slots_stdout.contains("latest_backend_evidence=none"));
    assert!(list_slots_stdout
        .contains("local-first-replay :: status=bounded-preview strategy=offline-replay"));
    assert!(list_slots_stdout
        .contains("adapter-intake-gate :: status=bounded-preview strategy=offline-replay"));
    assert!(list_slots_stdout.contains(
        "capability-parity-scan :: status=bounded-preview strategy=bounded-backend-comparison"
    ));

    let show_slot = run_vela(&vela_home, &["eval", "--show-slot", "adapter-intake-gate"]);
    assert!(show_slot.status.success(), "{}", stderr_text(&show_slot));
    let show_slot_stdout = stdout_text(&show_slot);
    assert!(show_slot_stdout.contains("backend experiment slot: id=adapter-intake-gate status=bounded-preview strategy=offline-replay"));
    assert!(show_slot_stdout.contains("backends=embedded,llamacpp,mock,ollama"));
    assert!(show_slot_stdout.contains("latest_eval_id=None"));
    assert!(show_slot_stdout.contains("latest_backends=none"));
    assert!(show_slot_stdout.contains("latest_passed=none"));
    assert!(show_slot_stdout.contains("latest_failed=none"));
    assert!(show_slot_stdout.contains("latest_capability_groups=none"));
    assert!(show_slot_stdout.contains("latest_results=0"));
    assert!(show_slot_stdout.contains("latest_backend_evidence=none"));
    assert!(show_slot_stdout.contains("hypothesis=Some("));

    let run_slot = run_vela(
        &vela_home,
        &[
            "eval",
            "--run-slot",
            "capability-parity-scan",
            "--backend",
            "mock",
            "--model",
            "mock-1",
        ],
    );
    assert!(run_slot.status.success(), "{}", stderr_text(&run_slot));
    let run_slot_stdout = stdout_text(&run_slot);
    assert!(run_slot_stdout.contains("slot=Some(\"capability-parity-scan\")"));
    assert!(run_slot_stdout.contains("parity_summary=Some(\""));
    assert!(run_slot_stdout.contains("parity=single-backend"));
    assert!(run_slot_stdout.contains("passed=mock"));
    assert!(run_slot_stdout.contains("capability_groups="));
    assert!(run_slot_stdout.contains("backend=mock transport=in-process status=passed"));

    let run_slot_second = run_vela(
        &vela_home,
        &[
            "eval",
            "--run-slot",
            "capability-parity-scan",
            "--backend",
            "mock",
            "--backend",
            "llamacpp",
            "--model",
            "mock-2",
        ],
    );
    assert!(
        run_slot_second.status.success(),
        "{}",
        stderr_text(&run_slot_second)
    );
    let run_slot_second_stdout = stdout_text(&run_slot_second);
    assert!(run_slot_second_stdout.contains("slot=Some(\"capability-parity-scan\")"));
    assert!(run_slot_second_stdout.contains("parity=diverged"));
    assert!(run_slot_second_stdout.contains("backend=mock transport=in-process status=passed"));
    assert!(run_slot_second_stdout.contains("backend=llamacpp transport=http-json status=failed"));

    let show_ran_slot = run_vela(
        &vela_home,
        &["eval", "--show-slot", "capability-parity-scan"],
    );
    assert!(
        show_ran_slot.status.success(),
        "{}",
        stderr_text(&show_ran_slot)
    );
    let show_ran_slot_stdout = stdout_text(&show_ran_slot);
    assert!(show_ran_slot_stdout.contains("backends=embedded,mock,llamacpp,ollama"));
    assert!(show_ran_slot_stdout.contains("latest_backends=mock,llamacpp"));
    assert!(show_ran_slot_stdout.contains("latest_passed=mock"));
    assert!(show_ran_slot_stdout.contains("latest_failed=llamacpp"));
    assert!(show_ran_slot_stdout.contains("latest_capability_groups=llamacpp=>text=true tool_loop=true reflection_retry=true images=false | mock=>text=true tool_loop=true reflection_retry=true images=true") || show_ran_slot_stdout.contains("latest_capability_groups=mock=>text=true tool_loop=true reflection_retry=true images=true | llamacpp=>text=true tool_loop=true reflection_retry=true images=false"));
    assert!(show_ran_slot_stdout.contains("latest_results=2"));
    assert!(show_ran_slot_stdout.contains("latest_parity_summary=Some(\"parity=diverged"));
    assert!(show_ran_slot_stdout.contains(
        "latest_backend_evidence=mock:passed@in-process source=runtime-mock model=mock-2; llamacpp:failed@http-json source=none model=mock-2"
    ));

    let list_slots_after = run_vela(&vela_home, &["eval", "--list-slots"]);
    assert!(
        list_slots_after.status.success(),
        "{}",
        stderr_text(&list_slots_after)
    );
    let list_slots_after_stdout = stdout_text(&list_slots_after);
    assert!(list_slots_after_stdout.contains(
        "capability-parity-scan :: status=bounded-preview strategy=bounded-backend-comparison"
    ));
    assert!(list_slots_after_stdout.contains("latest_passed=mock"));
    assert!(list_slots_after_stdout.contains("latest_failed=llamacpp"));
    assert!(list_slots_after_stdout.contains("latest_results=2"));
    assert!(list_slots_after_stdout.contains("latest_parity_summary=Some(\"parity=diverged"));
    assert!(list_slots_after_stdout.contains(
        "latest_backend_evidence=mock:passed@in-process source=runtime-mock model=mock-2; llamacpp:failed@http-json source=none model=mock-2"
    ));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that the backend eval harness records bounded provider failures without aborting the run.
fn backend_eval_harness_records_provider_failures() {
    let vela_home = temp_vela_home("eval-harness-failure");
    std::fs::create_dir_all(&vela_home).unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  model: phi-3-mini\n  llamacpp_base_url: http://10.0.0.15:8080\n",
    )
    .unwrap();

    let run = run_vela(
        &vela_home,
        &[
            "eval",
            "--run",
            "compare backend failure",
            "--backend",
            "llamacpp",
        ],
    );
    assert!(run.status.success(), "{}", stderr_text(&run));
    let run_stdout = stdout_text(&run);
    assert!(run_stdout.contains("backend=llamacpp transport=http-json status=failed"));
    assert!(run_stdout.contains("refusing non-local llama.cpp endpoint"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured mock provider is used for chat text turns.
fn chat_query_uses_configured_mock_provider() {
    let vela_home = temp_vela_home("mock-chat");
    std::fs::create_dir_all(&vela_home).unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: mock-1\n",
    )
    .unwrap();

    let turn = run_vela(&vela_home, &["chat", "--query", "hello from mock cli"]);
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Mock provider says hi."));
    assert!(turn_stdout.contains("response route: source=runtime-mock provider=mock model=mock-1 capabilities=text=true tool_loop=true reflection_retry=true images=true"));

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
/// Verifies that a configured llama.cpp provider can answer image turns through the text-only scaffold path.
fn chat_image_with_configured_llamacpp_provider_uses_text_only_scaffold_path() {
    let vela_home = temp_vela_home("llamacpp-image-scaffold");
    std::fs::create_dir_all(&vela_home).unwrap();
    let image_path = vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();
    let (base_url, server) = spawn_mock_llamacpp(
        "Phi scaffolded the image request.",
        "phi-3-mini",
        "The active backend cannot accept direct image bytes in this bounded contract",
    );
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: llamacpp\n  model: phi-3-mini\n  llamacpp_base_url: {}\n",
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
    assert!(turn_stdout.contains("Phi scaffolded the image request."));
    assert!(turn_stdout.contains("response route: source=runtime-llamacpp provider=llamacpp model=phi-3-mini capabilities=text=true tool_loop=true reflection_retry=true images=false"));
    server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured embedded provider can answer image turns through the text-only scaffold path.
fn chat_image_with_configured_embedded_provider_uses_text_only_scaffold_path() {
    let vela_home = temp_vela_home("embedded-image-scaffold");
    std::fs::create_dir_all(&vela_home).unwrap();
    let image_path = vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();
    let model_path = vela_home.join("models").join("gemma3.gguf");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"stub model").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: embedded\n  embedded_model_path: {}\n",
            model_path.display()
        ),
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &["chat", "--image", image_path.to_str().expect("image path")],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Embedded fixture reply."));
    assert!(turn_stdout.contains("response route: source=runtime-embedded provider=embedded capabilities=text=true tool_loop=true reflection_retry=true images=false"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured mock provider executes image turns through the provider path.
fn chat_image_uses_configured_mock_provider() {
    let vela_home = temp_vela_home("mock-image-fallback");
    std::fs::create_dir_all(&vela_home).unwrap();
    let image_path = vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: mock-1\n",
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &["chat", "--image", image_path.to_str().expect("image path")],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Mock provider inspected the image."));
    assert!(turn_stdout.contains("response route: source=runtime-mock provider=mock model=mock-1 capabilities=text=true tool_loop=true reflection_retry=true images=true"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured mock provider executes mixed text+image turns through the provider path.
fn chat_query_and_image_use_configured_mock_provider() {
    let vela_home = temp_vela_home("mock-mixed-image-turn");
    std::fs::create_dir_all(&vela_home).unwrap();
    let image_path = vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: mock-1\n",
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &[
            "chat",
            "--query",
            "summarize the mock diagram",
            "--image",
            image_path.to_str().expect("image path"),
        ],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout
        .contains("Mock provider inspected the image for request: summarize the mock diagram."));
    assert!(turn_stdout.contains("response route: source=runtime-mock provider=mock model=mock-1 capabilities=text=true tool_loop=true reflection_retry=true images=true"));

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
/// Verifies that a configured embedded provider can run the bounded tool loop through the CLI.
fn chat_query_uses_embedded_provider_tool_loop() {
    let vela_home = temp_vela_home("embedded-tool-loop");
    std::fs::create_dir_all(vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();
    let model_path = vela_home.join("models").join("gemma3.gguf");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"stub model").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: embedded\n  embedded_model_path: {}\n",
            model_path.display()
        ),
    )
    .unwrap();

    let turn = run_vela(&vela_home, &["chat", "--query", "need the tool loop"]);
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Embedded tool-informed final answer."));
    assert!(turn_stdout.contains("lifecycle: turn=turn-"));
    assert!(turn_stdout.contains("phases=8"));
    assert!(turn_stdout.contains("response route: source=runtime-embedded-tool-loop provider=embedded capabilities=text=true tool_loop=true reflection_retry=true images=false"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured mock provider can run the bounded tool loop during mixed text+image turns.
fn chat_query_and_image_use_mock_provider_tool_loop() {
    let vela_home = temp_vela_home("mock-image-tool-loop");
    std::fs::create_dir_all(vela_home.join("skills").join("deploy-staging")).unwrap();
    std::fs::write(
        vela_home
            .join("skills")
            .join("deploy-staging")
            .join("SKILL.md"),
        "# deploy-staging\n\nDeploys staging.",
    )
    .unwrap();
    let image_path = vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: mock-1\n",
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &[
            "chat",
            "--query",
            "need the tool loop for this image",
            "--image",
            image_path.to_str().expect("image path"),
        ],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Mock tool-informed final answer."));
    assert!(turn_stdout.contains("lifecycle: turn=turn-"));
    assert!(turn_stdout.contains("phases=8"));
    assert!(turn_stdout.contains("response route: source=runtime-mock-tool-loop provider=mock model=mock-1 capabilities=text=true tool_loop=true reflection_retry=true images=true"));

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

    let _vela_home = ScopedEnvVar::set("VELA_HOME", vela_home.as_os_str());
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
    server.join().unwrap();

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured embedded provider can recover from one invalid tool request through the CLI reflection path.
fn chat_query_recovers_from_invalid_tool_request_with_embedded_provider() {
    let vela_home = temp_vela_home("embedded-reflect-recover");
    std::fs::create_dir_all(&vela_home).unwrap();
    let model_path = vela_home.join("models").join("gemma3.gguf");
    std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
    std::fs::write(&model_path, b"stub model").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        format!(
            "runtime:\n  provider: embedded\n  embedded_model_path: {}\n",
            model_path.display()
        ),
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &["chat", "--query", "recover from invalid tool"],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Embedded recovered answer."));
    assert!(turn_stdout.contains("lifecycle: turn=turn-"));
    assert!(turn_stdout.contains("phases=6"));
    assert!(turn_stdout.contains("last=finish"));
    assert!(turn_stdout.contains("response route: source=runtime-embedded provider=embedded capabilities=text=true tool_loop=true reflection_retry=true images=false"));

    let _vela_home = ScopedEnvVar::set("VELA_HOME", vela_home.as_os_str());
    let bootstrap = vela_runtime::initialize_bootstrap(None, false).unwrap();
    let inspection = vela_runtime::inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("cli embedded reflection inspection");
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

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured mock provider can recover from one invalid tool request during a mixed text+image turn.
fn chat_query_and_image_recover_from_invalid_tool_request_with_mock_provider() {
    let vela_home = temp_vela_home("mock-image-reflect-recover");
    std::fs::create_dir_all(&vela_home).unwrap();
    let image_path = vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: mock-1\n",
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &[
            "chat",
            "--query",
            "recover from invalid tool in this image",
            "--image",
            image_path.to_str().expect("image path"),
        ],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("Mock recovered answer."));
    assert!(turn_stdout.contains("lifecycle: turn=turn-"));
    assert!(turn_stdout.contains("phases=6"));
    assert!(turn_stdout.contains("last=finish"));
    assert!(turn_stdout.contains("response route: source=runtime-mock provider=mock model=mock-1 capabilities=text=true tool_loop=true reflection_retry=true images=true"));

    let _vela_home = ScopedEnvVar::set("VELA_HOME", vela_home.as_os_str());
    let bootstrap = vela_runtime::initialize_bootstrap(None, false).unwrap();
    let inspection = vela_runtime::inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("cli mock image reflection inspection");
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
    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that a configured mock provider falls back after exhausting bounded reflection retries during a mixed text+image turn.
fn chat_query_and_image_fall_back_after_exhausting_reflection_retries_with_mock_provider() {
    let vela_home = temp_vela_home("mock-image-reflect-fallback");
    std::fs::create_dir_all(&vela_home).unwrap();
    let image_path = vela_home.join("diagram.png");
    std::fs::write(&image_path, b"fake-png-bytes").unwrap();
    std::fs::write(
        vela_home.join("config.yaml"),
        "runtime:\n  provider: mock\n  model: mock-1\n",
    )
    .unwrap();

    let turn = run_vela(
        &vela_home,
        &[
            "chat",
            "--query",
            "exhaust reflection retries in this image",
            "--image",
            image_path.to_str().expect("image path"),
        ],
    );
    assert!(turn.status.success(), "{}", stderr_text(&turn));
    let turn_stdout = stdout_text(&turn);
    assert!(turn_stdout.contains("exhausted the bounded reflection limit"));
    assert!(turn_stdout.contains("lifecycle: turn=turn-"));
    assert!(turn_stdout.contains("phases=9"));
    assert!(turn_stdout.contains("last=finish"));

    let _vela_home = ScopedEnvVar::set("VELA_HOME", vela_home.as_os_str());
    let bootstrap = vela_runtime::initialize_bootstrap(None, false).unwrap();
    let inspection = vela_runtime::inspect_latest_session(&bootstrap, 20)
        .unwrap()
        .expect("cli mock image reflection fallback inspection");
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
            "reflect",
            "retry",
            "reflect",
            "respond",
            "finish"
        ]
    );
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
    let parent_title = "chat: branch me";

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
    assert!(compress_stdout.contains("delta_messages="));
    assert!(compress_stdout.contains("delta_events="));

    let show = run_vela(&vela_home, &["sessions", "--show", branch_session]);
    assert!(show.status.success(), "{}", stderr_text(&show));
    let show_stdout = stdout_text(&show);
    assert!(show_stdout.contains("parent_id=Some"));
    assert!(show_stdout.contains("parent_title=Some"));
    assert!(show_stdout.contains("lineage [2]:"));
    assert!(show_stdout.contains(&format!("session={} title=chat: branch me", parent_session)));
    assert!(show_stdout.contains(&format!("session={} title=branch-a", branch_session)));
    assert!(show_stdout.contains("children [1]:"));
    assert!(show_stdout.contains("title=branch-a-child"));
    assert!(show_stdout.contains("descendants [1]:"));
    assert!(show_stdout.contains(&format!(
        "session={} title=branch-a-child",
        branch_child_session
    )));
    assert!(show_stdout.contains("compressions [1]:"));
    assert!(show_stdout.contains("delta_messages="));
    assert!(show_stdout.contains("delta_events="));
    assert!(show_stdout.contains("summary=branch compressed summary"));

    let compress_without_changes = run_vela(
        &vela_home,
        &[
            "sessions",
            "--compress",
            branch_session,
            "--summary",
            "branch follow-up summary",
        ],
    );
    assert!(!compress_without_changes.status.success());
    assert!(stderr_text(&compress_without_changes)
        .contains("compression requires new durable messages"));

    let branch_follow_up = run_vela(
        &vela_home,
        &[
            "chat",
            "--resume",
            branch_session,
            "--query",
            "branch follow-up",
        ],
    );
    assert!(
        branch_follow_up.status.success(),
        "{}",
        stderr_text(&branch_follow_up)
    );

    let compress_follow_up = run_vela(
        &vela_home,
        &[
            "sessions",
            "--compress",
            branch_session,
            "--summary",
            "branch follow-up summary",
        ],
    );
    assert!(
        compress_follow_up.status.success(),
        "{}",
        stderr_text(&compress_follow_up)
    );
    let compress_follow_up_stdout = stdout_text(&compress_follow_up);
    assert!(compress_follow_up_stdout.contains("delta_messages="));

    let branch_show_after_follow_up = run_vela(&vela_home, &["sessions", "--show", branch_session]);
    assert!(
        branch_show_after_follow_up.status.success(),
        "{}",
        stderr_text(&branch_show_after_follow_up)
    );
    let branch_show_after_follow_up_stdout = stdout_text(&branch_show_after_follow_up);
    assert!(branch_show_after_follow_up_stdout.contains("compressions [2]:"));
    assert!(branch_show_after_follow_up_stdout.contains("summary=branch follow-up summary"));
    assert!(branch_show_after_follow_up_stdout.contains("delta_messages="));

    let parent_show = run_vela(&vela_home, &["sessions", "--show", parent_session]);
    assert!(
        parent_show.status.success(),
        "{}",
        stderr_text(&parent_show)
    );
    let parent_show_stdout = stdout_text(&parent_show);
    assert!(parent_show_stdout.contains("children [2]:"));
    assert!(parent_show_stdout.contains("descendants [3]:"));
    assert!(parent_show_stdout.contains("title=branch-a"));
    assert!(parent_show_stdout.contains("title=branch-b"));
    assert!(parent_show_stdout.contains("title=branch-a-child"));

    let list = run_vela(&vela_home, &["sessions", "--list"]);
    assert!(list.status.success(), "{}", stderr_text(&list));
    let list_stdout = stdout_text(&list);
    assert!(list_stdout.contains("sessions [4]:"));
    assert!(list_stdout.contains(&format!(
        "depth=2 session={} title=branch-a-child",
        branch_child_session
    )));
    assert!(list_stdout.contains(&format!(
        "depth=1 session={} title=branch-a",
        branch_session
    )));
    assert!(list_stdout.contains(&format!(
        "depth=1 session={} title=branch-b",
        branch_b_session
    )));
    assert!(list_stdout.contains(&format!(
        "depth=0 session={} title=chat: branch me",
        parent_session
    )));

    let browse = run_vela(&vela_home, &["sessions", "--browse"]);
    assert!(browse.status.success(), "{}", stderr_text(&browse));
    let browse_stdout = stdout_text(&browse);
    assert!(browse_stdout.contains("session roots [1]:"));
    assert!(browse_stdout.contains(&format!(
        "root session={} title=chat: branch me",
        parent_session
    )));
    assert!(browse_stdout.contains("descendants [3]:"));
    assert!(browse_stdout.contains(&format!(
        "depth=2 session={} title=branch-a-child",
        branch_child_session
    )));
    assert!(browse_stdout.contains(&format!(
        "depth=1 session={} title=branch-a",
        branch_session
    )));
    assert!(browse_stdout.contains(&format!(
        "depth=1 session={} title=branch-b",
        branch_b_session
    )));

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
    assert!(continue_root_stdout
        .contains("continue resolution: mode=latest-descendant-of-anchor-title"));
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
    assert!(continue_branch_stdout
        .contains("continue resolution: mode=latest-descendant-of-anchor-title"));
    assert!(continue_branch_stdout.contains(&format!("anchor_id=Some(\"{}\")", branch_session)));

    let continue_exact = run_vela(
        &vela_home,
        &["chat", "--continue", "branch-b", "--query", "follow exact"],
    );
    assert!(
        continue_exact.status.success(),
        "{}",
        stderr_text(&continue_exact)
    );
    let continue_exact_stdout = stdout_text(&continue_exact);
    assert!(continue_exact_stdout.contains(&format!("id={}", branch_b_session)));
    assert!(continue_exact_stdout.contains("continue resolution: mode=exact-anchor-title"));

    let continue_by_id = run_vela(
        &vela_home,
        &[
            "chat",
            "--continue",
            branch_session,
            "--query",
            "follow exact id",
        ],
    );
    assert!(
        continue_by_id.status.success(),
        "{}",
        stderr_text(&continue_by_id)
    );
    let continue_by_id_stdout = stdout_text(&continue_by_id);
    assert!(continue_by_id_stdout.contains(&format!("id={}", branch_session)));
    assert!(continue_by_id_stdout.contains("continue resolution: mode=exact-session-id"));
    assert!(continue_by_id_stdout.contains("resolved_title=branch-a"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies cron job persistence and clap-level rejection of invalid flag combinations.
fn cron_registration_persists_and_invalid_flag_usage_is_rejected() {
    let vela_home = temp_vela_home("cron");

    let add = run_vela(
        &vela_home,
        &[
            "cron",
            "--add",
            "ping status",
            "--schedule",
            "0 * * * *",
            "--delivery-webhook-url",
            "https://example.test/hook",
            "--delivery-event-type",
            "scheduler.job.outcome",
        ],
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
    assert!(show_stdout.contains("progression=Some(\"registered\")"));
    assert!(show_stdout.contains("delivery_webhook_url=Some(\"https://example.test/hook\")"));
    assert!(show_stdout.contains("delivery_event_type=Some(\"scheduler.job.outcome\")"));

    let list = run_vela(&vela_home, &["cron", "--list"]);
    assert!(list.status.success(), "{}", stderr_text(&list));
    let list_stdout = stdout_text(&list);
    assert!(list_stdout.contains(job_id));
    assert!(list_stdout.contains("run_count=0"));
    assert!(list_stdout.contains("progression=Some(\"registered\")"));
    assert!(list_stdout.contains("delivery_event_type=Some(\"scheduler.job.outcome\")"));

    let invalid = run_vela(&vela_home, &["cron", "--schedule", "0 * * * *"]);
    assert!(!invalid.status.success());
    assert!(stderr_text(&invalid).contains("--add <ADD>"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that the scheduler report summarizes durable job and delivery state.
fn cron_report_summarizes_scheduler_state() {
    let vela_home = temp_vela_home("cron-report");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let add = run_vela(
        &vela_home,
        &[
            "cron",
            "--add",
            "ping status",
            "--schedule",
            "0 * * * *",
            "--delivery-webhook-url",
            "https://example.test/hook",
            "--delivery-event-type",
            "scheduler.job.outcome",
        ],
    );
    assert!(add.status.success(), "{}", stderr_text(&add));
    let add_stdout = stdout_text(&add);
    let job_id = parse_field(&add_stdout, "added:")
        .or_else(|| parse_field(&add_stdout, "job"))
        .unwrap_or_else(|| add_stdout.split_whitespace().nth(3).expect("job id token"))
        .to_string();

    let jobs_path = vela_home.join("scheduler").join("jobs.json");
    let mut jobs: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&jobs_path).unwrap()).unwrap();
    let job = jobs.as_array_mut().unwrap().first_mut().expect("first job");
    job["updated_at"] = serde_json::Value::from(16);
    job["run_count"] = serde_json::Value::from(2);
    job["recovery_count"] = serde_json::Value::from(1);
    job["last_started_at"] = serde_json::Value::from(11);
    job["last_completed_at"] = serde_json::Value::from(12);
    job["last_failed_at"] = serde_json::Value::from(15);
    job["last_recovered_at"] = serde_json::Value::from(13);
    job["last_session_id"] = serde_json::Value::from("session-123");
    job["execution_token"] = serde_json::Value::from("attempt-xyz");
    job["lease_expires_at"] = serde_json::Value::from(now + 21);
    job["last_outcome"] = serde_json::Value::from("completed");
    job["last_progression"] = serde_json::Value::from("completed-rescheduled");
    job["last_error"] = serde_json::Value::from(
        "temporary network failure while delivering scheduler webhook payload to downstream sink",
    );
    job["last_delivery_at"] = serde_json::Value::from(14);
    job["last_delivery_outcome"] = serde_json::Value::from("failed");
    job["last_delivery_error"] = serde_json::Value::from(
        "webhook delivery failed after retry because downstream rejected the payload body",
    );
    job["next_run_at"] = serde_json::Value::from(now + 42);
    std::fs::write(&jobs_path, serde_json::to_string_pretty(&jobs).unwrap()).unwrap();

    let report = run_vela(&vela_home, &["cron", "--report"]);
    assert!(report.status.success(), "{}", stderr_text(&report));
    let report_stdout = stdout_text(&report);
    assert!(report_stdout.contains("scheduler report:"));
    assert!(report_stdout.contains("jobs=1"));
    assert!(report_stdout.contains("pending=1"));
    assert!(report_stdout.contains("running=0"));
    assert!(report_stdout.contains("completed=1"));
    assert!(report_stdout.contains("failed=0"));
    assert!(report_stdout.contains("overdue=0"));
    assert!(report_stdout.contains("lease_expired=0"));
    assert!(report_stdout.contains("delivery_pending=0"));
    assert!(report_stdout.contains("delivery_failed=1"));
    assert!(report_stdout.contains("delivery_delivered=0"));
    assert!(report_stdout.contains("total_runs=2"));
    assert!(report_stdout.contains("total_recoveries=1"));
    assert!(report_stdout.contains(&format!("{}@{}", job_id, now + 42)));
    assert!(report_stdout.contains("scheduler jobs [1]:"));
    assert!(report_stdout.contains(&format!(
        "- {} :: schedule=0 * * * * source=scheduler status=pending",
        job_id
    )));
    assert!(report_stdout.contains("updated_at=16"));
    assert!(report_stdout.contains("due_state=scheduled"));
    assert!(report_stdout.contains("health_lag_seconds=None"));
    assert!(report_stdout.contains(&format!("lease_expires_at=Some({})", now + 21)));
    assert!(report_stdout.contains("last_run_at=Some(15)"));
    assert!(report_stdout.contains("last_completed_at=Some(12)"));
    assert!(report_stdout.contains("last_failed_at=Some(15)"));
    assert!(report_stdout.contains("last_recovered_at=Some(13)"));
    assert!(report_stdout.contains("last_session_id=Some(\"session-123\")"));
    assert!(report_stdout.contains("execution_token=Some(\"attempt-xyz\")"));
    assert!(report_stdout.contains("delivery_at=Some(14)"));
    assert!(report_stdout.contains("delivery_event_type=Some(\"scheduler.job.outcome\")"));
    assert!(report_stdout.contains("delivery_outcome=Some(\"failed\")"));
    assert!(report_stdout.contains("delivery_error_excerpt=Some(\"webhook delivery failed after retry because downstream rejected the payload body\""));
    assert!(report_stdout.contains("last_error_excerpt=Some(\"temporary network failure while delivering scheduler webhook payload"));

    let show = run_vela(&vela_home, &["cron", "--show", &job_id]);
    assert!(show.status.success(), "{}", stderr_text(&show));
    let show_stdout = stdout_text(&show);
    assert!(show_stdout.contains(&format!(
        "scheduled job: {} schedule=0 * * * * source=scheduler status=pending",
        job_id
    )));
    assert!(show_stdout.contains("updated_at=16"));
    assert!(show_stdout.contains("last_started_at=Some(11)"));
    assert!(show_stdout.contains("last_completed_at=Some(12)"));
    assert!(show_stdout.contains("last_failed_at=Some(15)"));
    assert!(show_stdout.contains("last_recovered_at=Some(13)"));
    assert!(show_stdout.contains(&format!("lease_expires_at=Some({})", now + 21)));
    assert!(show_stdout.contains("last_session_id=Some(\"session-123\")"));
    assert!(show_stdout.contains("execution_token=Some(\"attempt-xyz\")"));

    let mut delivered_jobs: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&jobs_path).unwrap()).unwrap();
    let delivered_job = delivered_jobs
        .as_array_mut()
        .unwrap()
        .first_mut()
        .expect("first job");
    delivered_job["last_delivery_outcome"] = serde_json::Value::from("delivered");
    delivered_job["last_delivery_error"] = serde_json::Value::Null;
    std::fs::write(
        &jobs_path,
        serde_json::to_string_pretty(&delivered_jobs).unwrap(),
    )
    .unwrap();

    let delivered_report = run_vela(&vela_home, &["cron", "--report"]);
    assert!(
        delivered_report.status.success(),
        "{}",
        stderr_text(&delivered_report)
    );
    let delivered_stdout = stdout_text(&delivered_report);
    assert!(delivered_stdout.contains("delivery_failed=0"));
    assert!(delivered_stdout.contains("delivery_delivered=1"));
    assert!(delivered_stdout.contains("delivery_outcome=Some(\"delivered\")"));
    assert!(delivered_stdout.contains("delivery_error_excerpt=None"));

    let mut overdue_jobs: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&jobs_path).unwrap()).unwrap();
    {
        let overdue_job = overdue_jobs
            .as_array_mut()
            .unwrap()
            .first_mut()
            .expect("first job");
        overdue_job["next_run_at"] = serde_json::Value::from(now - 30);
    }
    std::fs::write(
        &jobs_path,
        serde_json::to_string_pretty(&overdue_jobs).unwrap(),
    )
    .unwrap();

    let overdue_report = run_vela(&vela_home, &["cron", "--report"]);
    assert!(
        overdue_report.status.success(),
        "{}",
        stderr_text(&overdue_report)
    );
    let overdue_stdout = stdout_text(&overdue_report);
    assert!(overdue_stdout.contains("overdue=1"));
    assert!(overdue_stdout.contains("due_state=overdue"));
    assert!(overdue_stdout.contains("health_lag_seconds=Some("));

    {
        let overdue_job = overdue_jobs
            .as_array_mut()
            .unwrap()
            .first_mut()
            .expect("first job");
        overdue_job["status"] = serde_json::Value::from("running");
        overdue_job["lease_expires_at"] = serde_json::Value::from(now - 5);
    }
    std::fs::write(
        &jobs_path,
        serde_json::to_string_pretty(&overdue_jobs).unwrap(),
    )
    .unwrap();

    let lease_expired_report = run_vela(&vela_home, &["cron", "--report"]);
    assert!(
        lease_expired_report.status.success(),
        "{}",
        stderr_text(&lease_expired_report)
    );
    let lease_expired_stdout = stdout_text(&lease_expired_report);
    assert!(lease_expired_stdout.contains("lease_expired=1"));
    assert!(lease_expired_stdout.contains("due_state=lease-expired"));
    assert!(lease_expired_stdout.contains("health_lag_seconds=Some("));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
/// Verifies that starting the scheduler executes due jobs and records durable run metadata.
fn cron_start_executes_due_jobs() {
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    let vela_home = temp_vela_home("cron-start");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind scheduler webhook");
    listener
        .set_nonblocking(true)
        .expect("configure scheduler webhook listener");
    let url = format!(
        "http://{}{}",
        listener.local_addr().expect("scheduler webhook addr"),
        "/deliver"
    );
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(pair) => break pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for scheduler delivery"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept scheduler delivery: {error}"),
            }
        };
        let request = read_mock_http_request(&mut stream);
        let (_, body_text) = request.split_once("\r\n\r\n").expect("split headers/body");
        let payload: serde_json::Value =
            serde_json::from_str(body_text).expect("decode webhook JSON body");
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
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .expect("write scheduler webhook response");
    });

    let add = run_vela(
        &vela_home,
        &[
            "cron",
            "--add",
            "ping status",
            "--schedule",
            "* * * * *",
            "--delivery-webhook-url",
            &url,
            "--delivery-event-type",
            "scheduler.job.outcome",
        ],
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
    assert!(show_stdout.contains("progression=Some(\"completed-rescheduled\")"));
    assert!(show_stdout.contains("delivery_outcome=Some(\"delivered\")"));

    server.join().unwrap();
    std::fs::remove_dir_all(&vela_home).unwrap();
}
