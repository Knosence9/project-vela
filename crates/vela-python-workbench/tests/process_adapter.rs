use std::{fs, os::unix::fs::PermissionsExt};

use tempfile::tempdir;
use vela_python_workbench::{HamelnbProcessAdapter, PythonExecutionRequest};

#[test]
fn process_adapter_preserves_multiline_python_and_explicit_target() {
    let directory = tempdir().expect("adapter fixture directory");
    let adapter_path = directory.path().join("fake-hamelnb.py");
    fs::write(
        &adapter_path,
        r#"#!/usr/bin/env python3
import json
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
    )
    .expect("write fake adapter");
    let mut permissions = fs::metadata(&adapter_path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&adapter_path, permissions).unwrap();

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
