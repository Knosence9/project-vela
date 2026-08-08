use assert_cmd::Command;
use predicates::prelude::*;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
use tempfile::{TempDir, tempdir};

fn fake_adapter(body: &str) -> (TempDir, PathBuf) {
    let directory = tempdir().expect("adapter fixture directory");
    let adapter_path = directory.path().join("fake-hamelnb.py");
    fs::write(&adapter_path, format!("#!/usr/bin/env python3\n{body}"))
        .expect("write fake adapter");
    let mut permissions = fs::metadata(&adapter_path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&adapter_path, permissions).unwrap();
    (directory, adapter_path)
}

fn python_command(adapter_path: &std::path::Path) -> Command {
    let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
    command.args([
        "python",
        "execute",
        adapter_path.to_str().expect("UTF-8 adapter path"),
        "8888",
        "scratch.ipynb",
    ]);
    command
}

#[test]
fn python_execute_reads_multiline_source_from_stdin() {
    let (_directory, adapter_path) = fake_adapter(
        r#"import json
import sys

args = sys.argv[1:]
code_file_index = args.index("--code-file") + 1
with open(args[code_file_index], encoding="utf-8") as code_file:
    source = code_file.read()
print(json.dumps({
    "status": "ok",
    "transport": "websocket",
    "observed_code": source,
    "source_exposed_in_argv": source in args,
}, separators=(",", ":")))
"#,
    );

    let source = "values = [1, 2, 3]\nprint(sum(values))\n";
    python_command(&adapter_path)
        .write_stdin(source)
        .assert()
        .success()
        .stdout(concat!(
            "{\"observed_code\":\"values = [1, 2, 3]\\nprint(sum(values))\\n\",",
            "\"source_exposed_in_argv\":false,\"status\":\"ok\",",
            "\"transport\":\"websocket\"}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn python_execute_fails_closed_when_adapter_exits_unsuccessfully() {
    let (_directory, adapter_path) = fake_adapter(
        r#"import sys
print('{"status":"ok","partial":true}')
print('adapter failed', file=sys.stderr)
raise SystemExit(9)
"#,
    );

    python_command(&adapter_path)
        .write_stdin("40 + 2\n")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: python_execution_failed:"));
}

#[test]
fn python_execute_fails_closed_on_malformed_json() {
    let (_directory, adapter_path) = fake_adapter("print('not-json')\n");

    python_command(&adapter_path)
        .write_stdin("40 + 2\n")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: python_execution_failed:"));
}

#[test]
fn python_execute_fails_closed_on_non_ok_execution_status() {
    let (_directory, adapter_path) =
        fake_adapter("print('{\"status\":\"error\",\"events\":[{\"type\":\"error\"}]}')\n");

    python_command(&adapter_path)
        .write_stdin("raise RuntimeError('nope')\n")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: python_execution_failed:"));
}
