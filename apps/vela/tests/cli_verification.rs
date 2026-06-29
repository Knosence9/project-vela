use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Verifies that a default runtime session becomes visible through `vela status`.
#[test]
fn default_runtime_session_surfaces_in_status() {
    let vela_home = temp_vela_home("status");

    let first = run_vela(&vela_home, &[]);
    assert!(first.status.success(), "{}", stderr_text(&first));
    let first_stdout = stdout_text(&first);
    assert!(first_stdout.contains("runtime session: action=created"));

    let status = run_vela(&vela_home, &["status"]);
    assert!(status.status.success(), "{}", stderr_text(&status));
    let status_stdout = stdout_text(&status);
    assert!(status_stdout.contains("active session: id=session-"));

    std::fs::remove_dir_all(&vela_home).unwrap();
}

/// Verifies that repeated gateway starts reuse the same command-scoped session.
#[test]
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

/// Verifies cron job persistence and clap-level rejection of invalid flag combinations.
#[test]
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
