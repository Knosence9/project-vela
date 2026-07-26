use std::{error::Error, fs, time::Duration};

use serde_json::json;
use tempfile::tempdir;
use vela_extensions::{
    ComponentTool, ExtensionKind, ExtensionRegistry, ToolComponentInvocationError,
    ToolExecutionLimits, compile_tool_components, prepare_tool_artifacts,
};
use vela_kernel::tool::{Tool, ToolEffect};

const ECHO_COMPONENT: &str = r#"
(component
  (core module $guest
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 64))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $next
      global.get $next
      local.get 3
      i32.add
      global.set $next)
    (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
      i32.const 4
      local.get $ptr
      i32.store
      i32.const 8
      local.get $len
      i32.store
      i32.const 0))
  (core instance $guest (instantiate $guest))
  (type $outcome (result string (error string)))
  (type $invoke (func (param "input" string) (result $outcome)))
  (func $invoke (type $invoke)
    (canon lift (core func $guest "invoke")
      (memory $guest "memory")
      (realloc (func $guest "realloc"))))
  (export "invoke" (func $invoke)))
"#;

#[test]
fn metadata_is_inert_and_json_round_trips() {
    let inert = component_tool(
        "inert.tool",
        &start_trapping_component(),
        ToolExecutionLimits::default(),
    );
    assert_eq!(inert.id().as_str(), "inert.tool");
    assert_eq!(inert.effect(), ToolEffect::Pure);

    let mut tool = component_tool("echo.tool", ECHO_COMPONENT, ToolExecutionLimits::default());

    assert_eq!(tool.id().as_str(), "echo.tool");
    assert_eq!(tool.effect(), ToolEffect::Pure);
    assert_eq!(
        tool.invoke(&json!({"nested": [true, null, 3]}))
            .expect("valid JSON output"),
        json!({"nested": [true, null, 3]})
    );
}

#[test]
fn guest_errors_malformed_output_and_traps_fail_closed_with_sources() {
    let cases = [
        ("guest.tool", result_component(true, "guest refused")),
        ("malformed.tool", result_component(false, "not json")),
        ("trap.tool", trapping_component()),
    ];

    for (id, component) in cases {
        let mut tool = component_tool(id, &component, ToolExecutionLimits::default());
        let error = tool.invoke(&json!(null)).expect_err("invocation must fail");
        let adapter = error
            .source()
            .and_then(|source| source.downcast_ref::<ToolComponentInvocationError>())
            .expect("typed adapter error");
        assert!(adapter.source().is_some(), "{id} must preserve its source");
        match id {
            "guest.tool" => assert!(
                matches!(adapter, ToolComponentInvocationError::Guest { source }
                    if source.diagnostic() == "guest refused")
            ),
            "malformed.tool" => {
                assert!(matches!(
                    adapter,
                    ToolComponentInvocationError::Output { .. }
                ))
            }
            "trap.tool" => {
                assert!(matches!(
                    adapter,
                    ToolComponentInvocationError::Execution { .. }
                ))
            }
            _ => unreachable!("fixed test case"),
        }
    }
}

#[test]
fn each_invocation_gets_fresh_guest_state() {
    let mut tool = component_tool(
        "state.tool",
        &stateful_component(),
        ToolExecutionLimits::default(),
    );

    assert_eq!(tool.invoke(&json!(null)).expect("first call"), json!(1));
    assert_eq!(tool.invoke(&json!(null)).expect("second call"), json!(1));
}

#[test]
fn store_instance_memory_and_table_limits_terminate_invocation() {
    let cases = [
        (
            "instances.tool",
            ECHO_COMPONENT.to_owned(),
            ToolExecutionLimits {
                max_instances: 0,
                ..ToolExecutionLimits::default()
            },
        ),
        (
            "memories.tool",
            ECHO_COMPONENT.to_owned(),
            ToolExecutionLimits {
                max_memories: 0,
                ..ToolExecutionLimits::default()
            },
        ),
        (
            "memory-size.tool",
            ECHO_COMPONENT.to_owned(),
            ToolExecutionLimits {
                max_memory_bytes: 32 * 1024,
                ..ToolExecutionLimits::default()
            },
        ),
        (
            "tables.tool",
            table_component(1),
            ToolExecutionLimits {
                max_tables: 0,
                ..ToolExecutionLimits::default()
            },
        ),
        (
            "table-elements.tool",
            table_component(2),
            ToolExecutionLimits {
                max_table_elements: 1,
                ..ToolExecutionLimits::default()
            },
        ),
    ];

    for (id, component, limits) in cases {
        let mut tool = component_tool(id, &component, limits);
        assert_execution_error(&mut tool, id);
    }
}

#[test]
fn fuel_and_epoch_deadline_terminate_infinite_guests() {
    let mut fuel_tool = component_tool(
        "fuel.tool",
        &looping_component(),
        ToolExecutionLimits {
            fuel: 1_000,
            epoch_deadline: Duration::from_secs(10),
            ..ToolExecutionLimits::default()
        },
    );
    assert_execution_error(&mut fuel_tool, "fuel.tool");

    let mut epoch_tool = component_tool(
        "epoch.tool",
        &looping_component(),
        ToolExecutionLimits {
            fuel: u64::MAX,
            epoch_deadline: Duration::from_millis(10),
            ..ToolExecutionLimits::default()
        },
    );
    assert_execution_error(&mut epoch_tool, "epoch.tool");
}

fn assert_execution_error(tool: &mut ComponentTool, id: &str) {
    let error = tool.invoke(&json!(null)).expect_err("limit must fail");
    let adapter = error
        .source()
        .and_then(|source| source.downcast_ref::<ToolComponentInvocationError>())
        .expect("typed adapter error");
    assert!(
        matches!(adapter, ToolComponentInvocationError::Execution { .. }),
        "unexpected {id} error: {adapter:?}"
    );
    assert!(adapter.source().is_some());
}

fn component_tool(id: &str, wat: &str, limits: ToolExecutionLimits) -> ComponentTool {
    let root = tempdir().expect("temporary extension root");
    let package = root.path().join("tool");
    fs::create_dir(&package).expect("create package");
    fs::write(
        package.join("extension.yaml"),
        format!("manifest_version: 1\nid: {id}\nkind: tool\nentrypoint: run\n"),
    )
    .expect("write manifest");
    fs::write(
        package.join("run"),
        wat::parse_str(wat).expect("component WAT"),
    )
    .expect("write component");
    let registry = ExtensionRegistry::discover(root.path()).expect("registry");
    let selection = registry
        .select_kind(ExtensionKind::Tool, [id])
        .expect("tool selection");
    let artifacts = prepare_tool_artifacts(root.path(), &selection).expect("prepared artifact");
    let compiled = compile_tool_components(&artifacts).expect("compiled component");
    let component = compiled.into_iter().next().expect("one component");
    if limits == ToolExecutionLimits::default() {
        ComponentTool::new(component).expect("component tool")
    } else {
        ComponentTool::with_limits(component, limits).expect("component tool")
    }
}

fn result_component(is_error: bool, text: &str) -> String {
    let discriminant = u8::from(is_error);
    format!(
        r#"(component
          (core module $guest
            (memory (export "memory") 1)
            (data (i32.const 16) "{text}")
            (data (i32.const 0) "\{discriminant:02x}\00\00\00\10\00\00\00\{length:02x}\00\00\00")
            (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 64)
            (func (export "invoke") (param i32 i32) (result i32) i32.const 0))
          (core instance $guest (instantiate $guest))
          (type $outcome (result string (error string)))
          (type $invoke (func (param "input" string) (result $outcome)))
          (func $invoke (type $invoke) (canon lift (core func $guest "invoke")
            (memory $guest "memory") (realloc (func $guest "realloc"))))
          (export "invoke" (func $invoke)))"#,
        length = text.len(),
    )
}

fn stateful_component() -> String {
    result_component(false, "0").replace(
        "(func (export \"invoke\") (param i32 i32) (result i32) i32.const 0)",
        "(func (export \"invoke\") (param i32 i32) (result i32)\n             i32.const 16 i32.const 16 i32.load8_u i32.const 1 i32.add i32.store8\n             i32.const 0)",
    )
}

fn trapping_component() -> String {
    ECHO_COMPONENT.replace("i32.const 0))", "unreachable))")
}

fn start_trapping_component() -> String {
    ECHO_COMPONENT.replace(
        "(global $next",
        "(func $start unreachable) (start $start) (global $next",
    )
}

fn looping_component() -> String {
    ECHO_COMPONENT.replace("i32.const 0))", "(loop $forever br $forever) unreachable))")
}

fn table_component(elements: usize) -> String {
    ECHO_COMPONENT.replace(
        "(memory (export \"memory\") 1)",
        &format!("(memory (export \"memory\") 1) (table {elements} funcref)"),
    )
}
