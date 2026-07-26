use std::{error::Error, fs};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_extensions::{
    ExtensionKind, ExtensionRegistry, ToolActivationError, ToolComponentInvocationError,
    ToolDeactivationError, ToolExecutionLimits, activate_tool_selection,
    activate_tool_selection_with_limits, deactivate_tool_selection,
};
use vela_kernel::tool::{
    PermissionDecision, Tool, ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolInvocationError,
    ToolRegistry, ToolRegistryError, ToolRegistryInvocationError, ToolRegistryRemovalError,
    ToolRequest,
};

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
fn activates_exact_tools_without_running_guests() {
    let root = tempdir().expect("temporary extension root");
    write_tool(
        root.path(),
        "zeta",
        "zeta.tool",
        &start_trapping_component(),
    );
    write_tool(root.path(), "alpha", "alpha.tool", ECHO_COMPONENT);
    let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = extensions
        .select_kind(ExtensionKind::Tool, ["zeta.tool", "alpha.tool"])
        .expect("tool selection");
    let mut tools = ToolRegistry::new();

    activate_tool_selection(root.path(), &selection, &mut tools).expect("atomic activation");

    assert_eq!(
        tools
            .metadata()
            .into_iter()
            .map(|metadata| (metadata.id().as_str().to_owned(), metadata.effect()))
            .collect::<Vec<_>>(),
        vec![
            ("alpha.tool".to_owned(), ToolEffect::Pure),
            ("zeta.tool".to_owned(), ToolEffect::Pure),
        ]
    );
    assert_eq!(
        tools
            .invoke(
                &ToolId::new("alpha.tool").expect("tool ID"),
                &mut Allow,
                &json!({"ready": true}),
            )
            .expect("authorized invocation"),
        json!({"ready": true})
    );
}

#[test]
fn invalid_or_duplicate_batches_leave_the_registry_unchanged() {
    let cases = [
        ("duplicate", ECHO_COMPONENT, true),
        ("invalid", "not a component", false),
    ];

    for (case, component, duplicate) in cases {
        let root = tempdir().expect("temporary extension root");
        write_tool(root.path(), "first", "first.tool", ECHO_COMPONENT);
        write_tool(
            root.path(),
            "second",
            if duplicate {
                "existing.tool"
            } else {
                "second.tool"
            },
            component,
        );
        let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
        let ids = if duplicate {
            vec!["first.tool", "existing.tool"]
        } else {
            vec!["first.tool", "second.tool"]
        };
        let selection = extensions
            .select_kind(ExtensionKind::Tool, ids)
            .expect("tool selection");
        let mut tools = ToolRegistry::new();
        tools
            .register(DummyTool::new("existing.tool"))
            .expect("seed registry");

        let error = activate_tool_selection(root.path(), &selection, &mut tools)
            .expect_err("batch must fail closed");

        assert_eq!(
            tools
                .metadata()
                .into_iter()
                .map(|metadata| metadata.id().as_str().to_owned())
                .collect::<Vec<_>>(),
            vec!["existing.tool"]
        );
        match case {
            "duplicate" => assert!(matches!(
                error,
                ToolActivationError::Registration {
                    source: ToolRegistryError::DuplicateId { ref tool_id }
                } if tool_id.as_str() == "existing.tool"
            )),
            "invalid" => {
                assert!(matches!(error, ToolActivationError::Compilation { .. }));
                assert!(error.source().is_some());
            }
            _ => unreachable!("fixed cases"),
        }
    }
}

#[test]
fn explicit_limits_are_applied_only_when_an_activated_tool_is_invoked() {
    let root = tempdir().expect("temporary extension root");
    write_tool(root.path(), "limited", "limited.tool", ECHO_COMPONENT);
    let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = extensions
        .select_kind(ExtensionKind::Tool, ["limited.tool"])
        .expect("tool selection");
    let mut tools = ToolRegistry::new();

    activate_tool_selection_with_limits(
        root.path(),
        &selection,
        &mut tools,
        ToolExecutionLimits {
            max_instances: 0,
            ..ToolExecutionLimits::default()
        },
    )
    .expect("restrictive limits must not instantiate during activation");

    assert_eq!(tools.metadata().len(), 1);
    let error = tools
        .invoke(
            &ToolId::new("limited.tool").expect("tool ID"),
            &mut Allow,
            &json!({"ready": true}),
        )
        .expect_err("the caller-selected instance limit must terminate invocation");
    let ToolRegistryInvocationError::Invocation(ToolInvocationError::Tool { error, .. }) = error
    else {
        panic!("unexpected registry invocation error: {error:?}");
    };
    assert!(matches!(
        error
            .source()
            .and_then(|source| source.downcast_ref::<ToolComponentInvocationError>()),
        Some(ToolComponentInvocationError::Execution { .. })
    ));
}

#[test]
fn non_tool_selection_leaves_the_registry_unchanged() {
    let root = tempdir().expect("temporary extension root");
    write_tool(root.path(), "skill", "skill.one", ECHO_COMPONENT);
    fs::write(
        root.path().join("skill/extension.yaml"),
        "manifest_version: 1\nid: skill.one\nkind: skill\nentrypoint: run\n",
    )
    .expect("rewrite skill manifest");
    let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = extensions.select(["skill.one"]).expect("generic selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(DummyTool::new("existing.tool"))
        .expect("seed registry");

    let error = activate_tool_selection(root.path(), &selection, &mut tools)
        .expect_err("unsupported kind must fail closed");

    assert!(matches!(error, ToolActivationError::Preparation { .. }));
    assert_eq!(tools.metadata().len(), 1);
}

#[test]
fn empty_selection_is_a_noop() {
    let root = tempdir().expect("temporary extension root");
    let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = extensions
        .select_kind(ExtensionKind::Tool, std::iter::empty::<&str>())
        .expect("empty selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(DummyTool::new("existing.tool"))
        .expect("seed registry");
    fs::remove_dir(root.path()).expect("remove unused empty root");

    activate_tool_selection_with_limits(
        root.path(),
        &selection,
        &mut tools,
        ToolExecutionLimits {
            max_instances: 0,
            ..ToolExecutionLimits::default()
        },
    )
    .expect("empty activation");
    deactivate_tool_selection(&selection, &mut tools).expect("empty deactivation");

    assert_eq!(tools.metadata().len(), 1);
}

#[test]
fn selected_tools_are_deactivated_atomically_without_filesystem_or_guest_access() {
    let root = tempdir().expect("temporary extension root");
    write_tool(
        root.path(),
        "alpha",
        "alpha.tool",
        &start_trapping_component(),
    );
    write_tool(root.path(), "zeta", "zeta.tool", ECHO_COMPONENT);
    let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let alpha = extensions
        .select_kind(ExtensionKind::Tool, ["alpha.tool"])
        .expect("alpha selection");
    let both = extensions
        .select_kind(ExtensionKind::Tool, ["alpha.tool", "zeta.tool"])
        .expect("full selection");
    let mut tools = ToolRegistry::new();
    activate_tool_selection(root.path(), &alpha, &mut tools).expect("alpha activation");
    fs::remove_dir_all(root.path()).expect("remove extension root");

    let error = deactivate_tool_selection(&both, &mut tools)
        .expect_err("missing zeta must preserve alpha atomically");
    assert!(error.source().is_some());
    assert!(matches!(
        error,
        ToolDeactivationError::Registry {
            source: ToolRegistryRemovalError::NotFound { ref tool_id }
        } if tool_id.as_str() == "zeta.tool"
    ));
    assert_eq!(tools.metadata()[0].id().as_str(), "alpha.tool");

    deactivate_tool_selection(&alpha, &mut tools).expect("deactivation without root access");
    assert!(tools.metadata().is_empty());
    let error = tools
        .invoke(
            &ToolId::new("alpha.tool").expect("tool ID"),
            &mut Allow,
            &json!(null),
        )
        .expect_err("deactivated tool must not resolve");
    assert!(matches!(
        error,
        ToolRegistryInvocationError::NotFound { .. }
    ));
}

#[test]
fn non_tool_selection_cannot_deactivate_a_same_id_adapter() {
    let root = tempdir().expect("temporary extension root");
    let package = root.path().join("skill");
    fs::create_dir(&package).expect("create package");
    fs::write(
        package.join("extension.yaml"),
        "manifest_version: 1\nid: shared.id\nkind: skill\nentrypoint: skill.md\n",
    )
    .expect("write manifest");
    fs::write(package.join("skill.md"), "instructions").expect("write entrypoint");
    let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let skill = extensions.select(["shared.id"]).expect("skill selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(DummyTool::new("shared.id"))
        .expect("register same-ID adapter");

    let error = deactivate_tool_selection(&skill, &mut tools)
        .expect_err("wrong-kind metadata must fail closed");

    assert!(matches!(
        error,
        ToolDeactivationError::WrongKind {
            ref id,
            actual: ExtensionKind::Skill
        } if id == "shared.id"
    ));
    assert!(error.source().is_none());
    assert_eq!(tools.metadata()[0].id().as_str(), "shared.id");
}

fn write_tool(root: &std::path::Path, package_name: &str, id: &str, component: &str) {
    let package = root.join(package_name);
    fs::create_dir(&package).expect("create package");
    fs::write(
        package.join("extension.yaml"),
        format!("manifest_version: 1\nid: {id}\nkind: tool\nentrypoint: run\n"),
    )
    .expect("write manifest");
    let bytes = wat::parse_str(component).unwrap_or_else(|_| component.as_bytes().to_vec());
    fs::write(package.join("run"), bytes).expect("write component");
}

fn start_trapping_component() -> String {
    ECHO_COMPONENT.replace(
        "(global $next",
        "(func $start unreachable) (start $start) (global $next",
    )
}

struct Allow;

impl ToolAuthorizer for Allow {
    fn authorize(&mut self, _request: ToolRequest<'_>) -> PermissionDecision {
        PermissionDecision::Allow
    }
}

struct DummyTool {
    id: ToolId,
}

impl DummyTool {
    fn new(id: &str) -> Self {
        Self {
            id: ToolId::new(id).expect("tool ID"),
        }
    }
}

impl Tool for DummyTool {
    fn id(&self) -> &ToolId {
        &self.id
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::Pure
    }

    fn invoke(&mut self, input: &Value) -> Result<Value, ToolError> {
        Ok(input.clone())
    }
}
