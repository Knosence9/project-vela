use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Spawns a one-shot mock Ollama server that returns a fixed response.
fn spawn_mock_ollama(response_body: &str) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock ollama");
    let addr = format!("http://{}", listener.local_addr().expect("mock ollama addr"));
    let body = response_body.to_string();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept mock ollama request");
        let mut buf = [0u8; 8192];
        let _ = stream.read(&mut buf);
        let payload = format!("{{\"response\":\"{}\"}}", body);
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(reply.as_bytes()).expect("write mock ollama response");
        stream.flush().expect("flush mock ollama response");
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
    let (base_url, server) = spawn_mock_ollama("Gemma local reply.");
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
/// Verifies cron job persistence and clap-level rejection of invalid flag combinations.
fn cron_registration_persists_and_invalid_flag_usage_is_rejected() {
    let vela_home = temp_vela_home("cron");

    let add = run_vela(&vela_home, &["cron", "--add", "ping status", "--schedule", "0 * * * *"]);
    assert!(add.status.success(), "{}", stderr_text(&add));
    let add_stdout = stdout_text(&add);
    let job_id = parse_field(&add_stdout, "added:").or_else(|| parse_field(&add_stdout, "job")).unwrap_or_else(|| {
        add_stdout
            .split_whitespace()
            .nth(3)
            .expect("job id token")
    });

    let show = run_vela(&vela_home, &["cron", "--show", job_id]);
    assert!(show.status.success(), "{}", stderr_text(&show));
    let show_stdout = stdout_text(&show);
    assert!(show_stdout.contains(job_id));
    assert!(show_stdout.contains("task=ping status"));

    let list = run_vela(&vela_home, &["cron", "--list"]);
    assert!(list.status.success(), "{}", stderr_text(&list));
    let list_stdout = stdout_text(&list);
    assert!(list_stdout.contains(job_id));

    let invalid = run_vela(&vela_home, &["cron", "--schedule", "0 * * * *"]);
    assert!(!invalid.status.success());
    assert!(stderr_text(&invalid).contains("--add <ADD>"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}
