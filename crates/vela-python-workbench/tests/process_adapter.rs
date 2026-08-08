use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

use tempfile::{TempDir, tempdir};
use vela_python_workbench::{
    HamelnbProcessAdapter, PythonExecutionError, PythonExecutionLimits, PythonExecutionRequest,
};

fn executable_adapter(body: &str) -> (TempDir, PathBuf) {
    let directory = tempdir().expect("adapter fixture directory");
    let adapter_path = directory.path().join("fake-hamelnb.py");
    let mut adapter = fs::File::create(&adapter_path).expect("create fake adapter");
    use std::io::Write as _;
    adapter
        .write_all(format!("#!/usr/bin/env python3\n{body}").as_bytes())
        .expect("write fake adapter");
    adapter.sync_all().expect("sync fake adapter");
    drop(adapter);
    let mut permissions = fs::metadata(&adapter_path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&adapter_path, permissions).unwrap();
    (directory, adapter_path)
}

#[test]
fn process_adapter_preserves_multiline_python_and_explicit_target() {
    let (_directory, adapter_path) = executable_adapter(
        r#"import json
import sys

args = sys.argv[1:]
code_file_index = args.index("--code-file") + 1
port_index = args.index("--port") + 1
path_index = args.index("--path") + 1
with open(args[code_file_index], encoding="utf-8") as code_file:
    source = code_file.read()
print(json.dumps({
    "status": "ok",
    "transport": "websocket",
    "observed_code": source,
    "source_exposed_in_argv": source in args,
    "observed_port": args[port_index],
    "observed_path": args[path_index],
}, separators=(",", ":")))
"#,
    );

    let source = "values = [1, 2, 3]\nprint(sum(values))\n";
    let request = PythonExecutionRequest::new(8888, "scratch.ipynb", source).unwrap();
    let result = HamelnbProcessAdapter::new(&adapter_path)
        .execute(&request)
        .expect("successful execution");

    assert_eq!(result.status(), "ok");
    assert_eq!(result.transport(), Some("websocket"));
    assert_eq!(result.value()["observed_code"], source);
    assert_eq!(result.value()["source_exposed_in_argv"], false);
    assert_eq!(result.value()["observed_port"], "8888");
    assert_eq!(result.value()["observed_path"], "scratch.ipynb");
}

#[test]
fn process_adapter_kills_a_direct_child_after_the_runtime_budget() {
    let (_directory, adapter_path) =
        executable_adapter("import time\ntime.sleep(30)\nprint('{\"status\":\"ok\"}')\n");
    let limits = PythonExecutionLimits::new(Duration::from_millis(50), 1024).unwrap();
    let request = PythonExecutionRequest::new(8888, "scratch.ipynb", "40 + 2")
        .unwrap()
        .with_limits(limits);

    let started = std::time::Instant::now();
    let error = HamelnbProcessAdapter::new(&adapter_path)
        .execute(&request)
        .expect_err("sleeping adapter must time out");

    assert!(matches!(error, PythonExecutionError::TimedOut { .. }));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn process_adapter_timeout_is_not_held_open_by_inherited_pipes() {
    let (_directory, adapter_path) = executable_adapter(
        r#"import subprocess
import sys
subprocess.Popen([sys.executable, "-c", "import time; time.sleep(0.1)"])
print('{"status":"ok"}')
"#,
    );
    let limits = PythonExecutionLimits::new(Duration::from_millis(20), 1024).unwrap();
    let request = PythonExecutionRequest::new(8888, "scratch.ipynb", "40 + 2")
        .unwrap()
        .with_limits(limits);

    let started = std::time::Instant::now();
    let error = HamelnbProcessAdapter::new(&adapter_path)
        .execute(&request)
        .expect_err("inherited pipes must not defeat the capture timeout");

    assert!(
        matches!(error, PythonExecutionError::TimedOut { .. }),
        "unexpected error: {error:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(150));
}

#[test]
fn process_adapter_rejects_stdout_beyond_the_capture_budget() {
    let (_directory, adapter_path) = executable_adapter("print('x' * 4096)\n");
    let limits = PythonExecutionLimits::new(Duration::from_secs(1), 128).unwrap();
    let request = PythonExecutionRequest::new(8888, "scratch.ipynb", "40 + 2")
        .unwrap()
        .with_limits(limits);

    let error = HamelnbProcessAdapter::new(&adapter_path)
        .execute(&request)
        .expect_err("oversized stdout must fail");

    assert!(
        matches!(
            error,
            PythonExecutionError::OutputLimitExceeded {
                stream: "stdout",
                limit: 128
            }
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn continuous_output_cannot_starve_the_runtime_budget() {
    let (_directory, adapter_path) = executable_adapter(
        "import sys\nwhile True:\n    sys.stdout.write('x' * 8192)\n    sys.stdout.flush()\n",
    );
    let limits = PythonExecutionLimits::new(Duration::from_millis(50), 100 * 1024 * 1024).unwrap();
    let request = PythonExecutionRequest::new(8888, "scratch.ipynb", "40 + 2")
        .unwrap()
        .with_limits(limits);

    let started = std::time::Instant::now();
    let error = HamelnbProcessAdapter::new(&adapter_path)
        .execute(&request)
        .expect_err("continuous output must not starve the deadline");

    assert!(matches!(error, PythonExecutionError::TimedOut { .. }));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn process_adapter_rejects_stderr_beyond_the_capture_budget() {
    let (_directory, adapter_path) =
        executable_adapter("import sys\nprint('x' * 4096, file=sys.stderr)\nraise SystemExit(2)\n");
    let limits = PythonExecutionLimits::new(Duration::from_secs(1), 128).unwrap();
    let request = PythonExecutionRequest::new(8888, "scratch.ipynb", "40 + 2")
        .unwrap()
        .with_limits(limits);

    let error = HamelnbProcessAdapter::new(&adapter_path)
        .execute(&request)
        .expect_err("oversized stderr must fail");

    assert!(matches!(
        error,
        PythonExecutionError::OutputLimitExceeded {
            stream: "stderr",
            limit: 128
        }
    ));
}

#[test]
fn execution_limits_reject_zero_budgets() {
    assert!(PythonExecutionLimits::new(Duration::ZERO, 1).is_err());
    assert!(PythonExecutionLimits::new(Duration::from_secs(1), 0).is_err());
}
