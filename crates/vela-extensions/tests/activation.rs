use std::{error::Error, fs};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_extensions::{
    ExtensionKind, ExtensionRegistry, ToolActivationError, ToolComponentInvocationError,
    ToolDeactivationError, ToolExecutionLimits, ToolReconciliationError, ToolReplacementError,
    activate_tool_selection, activate_tool_selection_with_limits, deactivate_tool_selection,
    reconcile_tool_selections, reconcile_tool_selections_with_limits, replace_tool_selection,
    replace_tool_selection_with_limits,
};
use vela_kernel::tool::{
    PermissionDecision, Tool, ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolInvocationError,
    ToolRegistry, ToolRegistryError, ToolRegistryInvocationError, ToolRegistryReconciliationError,
    ToolRegistryRemovalError, ToolRegistryReplacementError, ToolRequest,
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
    replace_tool_selection(root.path(), &selection, &mut tools).expect("empty replacement");
    reconcile_tool_selections(root.path(), &selection, &selection, &mut tools)
        .expect("empty reconciliation");
    reconcile_tool_selections_with_limits(
        root.path(),
        &selection,
        &selection,
        &mut tools,
        ToolExecutionLimits {
            max_instances: 0,
            ..ToolExecutionLimits::default()
        },
    )
    .expect("empty limited reconciliation");
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

#[test]
fn selected_active_tools_are_replaced_atomically_without_running_guests() {
    let root = tempdir().expect("temporary extension root");
    write_tool(
        root.path(),
        "alpha",
        "alpha.tool",
        &start_trapping_component(),
    );
    let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = extensions
        .select_kind(ExtensionKind::Tool, ["alpha.tool"])
        .expect("tool selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(MarkerTool::new("alpha.tool", json!({"adapter": "old"})))
        .expect("seed old adapter");

    replace_tool_selection_with_limits(
        root.path(),
        &selection,
        &mut tools,
        ToolExecutionLimits::default(),
    )
    .expect("atomic replacement");

    let error = tools
        .invoke(
            &ToolId::new("alpha.tool").expect("tool ID"),
            &mut Allow,
            &json!({"adapter": "new"}),
        )
        .expect_err("the newly installed guest must trap only when later invoked");
    assert!(matches!(
        error,
        ToolRegistryInvocationError::Invocation(ToolInvocationError::Tool { .. })
    ));
}

#[test]
fn selected_tool_replacement_applies_explicit_limits_only_on_later_invocation() {
    let root = tempdir().expect("temporary extension root");
    write_tool(root.path(), "limited", "limited.tool", ECHO_COMPONENT);
    let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = extensions
        .select_kind(ExtensionKind::Tool, ["limited.tool"])
        .expect("tool selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(MarkerTool::new("limited.tool", json!({"adapter": "old"})))
        .expect("seed old adapter");

    replace_tool_selection_with_limits(
        root.path(),
        &selection,
        &mut tools,
        ToolExecutionLimits {
            max_instances: 0,
            ..ToolExecutionLimits::default()
        },
    )
    .expect("restrictive limits must not instantiate during replacement");

    let error = tools
        .invoke(
            &ToolId::new("limited.tool").expect("tool ID"),
            &mut Allow,
            &json!(null),
        )
        .expect_err("replacement invocation must receive the explicit limit");
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
fn failed_selected_tool_replacement_preserves_the_old_invocable_batch() {
    for (case, component) in [("invalid", "not a component"), ("missing", ECHO_COMPONENT)] {
        let root = tempdir().expect("temporary extension root");
        write_tool(root.path(), "alpha", "alpha.tool", component);
        let extensions = ExtensionRegistry::discover(root.path()).expect("extension registry");
        let selection = extensions
            .select_kind(ExtensionKind::Tool, ["alpha.tool"])
            .expect("tool selection");
        let mut tools = ToolRegistry::new();
        if case == "invalid" {
            tools
                .register(MarkerTool::new("alpha.tool", json!({"adapter": "old"})))
                .expect("seed old adapter");
        } else {
            tools
                .register(MarkerTool::new("keep.tool", json!({"adapter": "keep"})))
                .expect("seed unrelated adapter");
        }

        let error = replace_tool_selection(root.path(), &selection, &mut tools)
            .expect_err("replacement must fail closed");

        match case {
            "invalid" => assert!(matches!(error, ToolReplacementError::Compilation { .. })),
            "missing" => assert!(matches!(
                error,
                ToolReplacementError::Registry {
                    source: ToolRegistryReplacementError::NotFound { ref tool_id }
                } if tool_id.as_str() == "alpha.tool"
            )),
            _ => unreachable!("fixed cases"),
        }
        let existing_id = if case == "invalid" {
            "alpha.tool"
        } else {
            "keep.tool"
        };
        let expected = if case == "invalid" {
            json!({"adapter": "old"})
        } else {
            json!({"adapter": "keep"})
        };
        assert_eq!(
            tools
                .invoke(
                    &ToolId::new(existing_id).expect("tool ID"),
                    &mut Allow,
                    &json!(null),
                )
                .expect("old adapter remains invocable"),
            expected
        );
    }
}

#[test]
fn sequential_deactivation_then_failed_activation_exposes_partial_state() {
    let previous_root = tempdir().expect("previous extension root");
    write_tool(previous_root.path(), "alpha", "alpha.tool", ECHO_COMPONENT);
    let previous_extensions =
        ExtensionRegistry::discover(previous_root.path()).expect("previous registry");
    let previous = previous_extensions
        .select_kind(ExtensionKind::Tool, ["alpha.tool"])
        .expect("previous selection");
    let current_root = tempdir().expect("current extension root");
    write_tool(
        current_root.path(),
        "alpha",
        "alpha.tool",
        "not a component",
    );
    let current_extensions =
        ExtensionRegistry::discover(current_root.path()).expect("current registry");
    let current = current_extensions
        .select_kind(ExtensionKind::Tool, ["alpha.tool"])
        .expect("current selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(MarkerTool::new("alpha.tool", json!({"adapter": "old"})))
        .expect("old adapter");

    deactivate_tool_selection(&previous, &mut tools).expect("first sequential mutation");
    let error = activate_tool_selection(current_root.path(), &current, &mut tools)
        .expect_err("second sequential operation fails");

    assert!(matches!(error, ToolActivationError::Compilation { .. }));
    assert!(matches!(
        tools
            .invoke(
                &ToolId::new("alpha.tool").unwrap(),
                &mut Allow,
                &json!(null),
            )
            .expect_err("old adapter was already removed"),
        ToolRegistryInvocationError::NotFound { .. }
    ));
}

#[test]
fn selected_tool_reconciliation_is_one_atomic_remove_replace_add_transition() {
    let previous_root = tempdir().expect("previous extension root");
    write_tool(previous_root.path(), "alpha", "alpha.tool", ECHO_COMPONENT);
    write_tool(
        previous_root.path(),
        "remove",
        "remove.tool",
        ECHO_COMPONENT,
    );
    let previous_extensions =
        ExtensionRegistry::discover(previous_root.path()).expect("previous registry");
    let previous = previous_extensions
        .select_kind(ExtensionKind::Tool, ["alpha.tool", "remove.tool"])
        .expect("previous selection");

    let current_root = tempdir().expect("current extension root");
    write_tool(current_root.path(), "alpha", "alpha.tool", ECHO_COMPONENT);
    write_tool(current_root.path(), "added", "added.tool", ECHO_COMPONENT);
    let current_extensions =
        ExtensionRegistry::discover(current_root.path()).expect("current registry");
    let current = current_extensions
        .select_kind(ExtensionKind::Tool, ["alpha.tool", "added.tool"])
        .expect("current selection");

    let mut tools = ToolRegistry::new();
    tools
        .register(MarkerTool::new("alpha.tool", json!({"adapter": "old"})))
        .expect("old alpha");
    tools
        .register(MarkerTool::new("remove.tool", json!({"adapter": "remove"})))
        .expect("removed tool");
    tools
        .register(MarkerTool::new("keep.tool", json!({"adapter": "keep"})))
        .expect("unrelated tool");

    reconcile_tool_selections_with_limits(
        current_root.path(),
        &previous,
        &current,
        &mut tools,
        ToolExecutionLimits::default(),
    )
    .expect("atomic reconciliation");

    assert_eq!(
        tools
            .metadata()
            .iter()
            .map(|metadata| metadata.id().as_str())
            .collect::<Vec<_>>(),
        vec!["added.tool", "alpha.tool", "keep.tool"]
    );
    for id in ["added.tool", "alpha.tool"] {
        let input = json!({"tool": id});
        assert_eq!(
            tools
                .invoke(&ToolId::new(id).unwrap(), &mut Allow, &input)
                .expect("new component invocation"),
            input
        );
    }
    assert_eq!(
        tools
            .invoke(&ToolId::new("keep.tool").unwrap(), &mut Allow, &json!(null),)
            .expect("unrelated adapter remains"),
        json!({"adapter": "keep"})
    );
    assert!(matches!(
        tools
            .invoke(
                &ToolId::new("remove.tool").unwrap(),
                &mut Allow,
                &json!(null),
            )
            .expect_err("removed adapter"),
        ToolRegistryInvocationError::NotFound { .. }
    ));
}

#[test]
fn failed_selected_tool_reconciliation_preserves_every_old_adapter() {
    let previous_root = tempdir().expect("previous extension root");
    write_tool(previous_root.path(), "alpha", "alpha.tool", ECHO_COMPONENT);
    let previous_extensions =
        ExtensionRegistry::discover(previous_root.path()).expect("previous registry");
    let previous = previous_extensions
        .select_kind(ExtensionKind::Tool, ["alpha.tool"])
        .expect("previous selection");

    let current_root = tempdir().expect("current extension root");
    write_tool(
        current_root.path(),
        "alpha",
        "alpha.tool",
        "not a component",
    );
    let current_extensions =
        ExtensionRegistry::discover(current_root.path()).expect("current registry");
    let current = current_extensions
        .select_kind(ExtensionKind::Tool, ["alpha.tool"])
        .expect("current selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(MarkerTool::new("alpha.tool", json!({"adapter": "old"})))
        .expect("old adapter");

    let error = reconcile_tool_selections(current_root.path(), &previous, &current, &mut tools)
        .expect_err("invalid current component must fail before mutation");

    assert!(matches!(error, ToolReconciliationError::Compilation { .. }));
    assert_eq!(
        tools
            .invoke(
                &ToolId::new("alpha.tool").unwrap(),
                &mut Allow,
                &json!(null),
            )
            .expect("old adapter remains"),
        json!({"adapter": "old"})
    );
}

#[test]
fn selected_tool_reconciliation_reports_registry_conflicts_without_mutation() {
    let previous_root = tempdir().expect("previous extension root");
    let previous_extensions =
        ExtensionRegistry::discover(previous_root.path()).expect("previous registry");
    let previous = previous_extensions
        .select_kind(ExtensionKind::Tool, std::iter::empty::<&str>())
        .expect("empty previous selection");
    let current_root = tempdir().expect("current extension root");
    write_tool(current_root.path(), "keep", "keep.tool", ECHO_COMPONENT);
    let current_extensions =
        ExtensionRegistry::discover(current_root.path()).expect("current registry");
    let current = current_extensions
        .select_kind(ExtensionKind::Tool, ["keep.tool"])
        .expect("current selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(MarkerTool::new(
            "keep.tool",
            json!({"adapter": "unrelated"}),
        ))
        .expect("unrelated adapter");

    let error = reconcile_tool_selections(current_root.path(), &previous, &current, &mut tools)
        .expect_err("current-only collision");

    assert!(matches!(
        error,
        ToolReconciliationError::Registry {
            source: ToolRegistryReconciliationError::CurrentConflict { ref tool_id }
        } if tool_id.as_str() == "keep.tool"
    ));
    assert_eq!(
        tools
            .invoke(&ToolId::new("keep.tool").unwrap(), &mut Allow, &json!(null),)
            .expect("unrelated adapter remains"),
        json!({"adapter": "unrelated"})
    );
}

#[test]
fn selected_tool_reconciliation_applies_limits_only_on_later_invocation() {
    let previous_root = tempdir().expect("previous extension root");
    write_tool(
        previous_root.path(),
        "limited",
        "limited.tool",
        ECHO_COMPONENT,
    );
    let previous_extensions =
        ExtensionRegistry::discover(previous_root.path()).expect("previous registry");
    let previous = previous_extensions
        .select_kind(ExtensionKind::Tool, ["limited.tool"])
        .expect("previous selection");
    let current_root = tempdir().expect("current extension root");
    write_tool(
        current_root.path(),
        "limited",
        "limited.tool",
        ECHO_COMPONENT,
    );
    let current_extensions =
        ExtensionRegistry::discover(current_root.path()).expect("current registry");
    let current = current_extensions
        .select_kind(ExtensionKind::Tool, ["limited.tool"])
        .expect("current selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(MarkerTool::new("limited.tool", json!({"adapter": "old"})))
        .expect("old adapter");

    reconcile_tool_selections_with_limits(
        current_root.path(),
        &previous,
        &current,
        &mut tools,
        ToolExecutionLimits {
            max_instances: 0,
            ..ToolExecutionLimits::default()
        },
    )
    .expect("restrictive limits remain inert during reconciliation");

    let error = tools
        .invoke(
            &ToolId::new("limited.tool").unwrap(),
            &mut Allow,
            &json!(null),
        )
        .expect_err("later invocation receives the configured limit");
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
fn non_tool_selection_cannot_participate_in_tool_reconciliation() {
    let previous_root = tempdir().expect("previous extension root");
    let package = previous_root.path().join("skill");
    fs::create_dir(&package).expect("skill package");
    fs::write(
        package.join("extension.yaml"),
        "manifest_version: 1\nid: shared.id\nkind: skill\nentrypoint: skill.md\n",
    )
    .expect("skill manifest");
    fs::write(package.join("skill.md"), "instructions").expect("skill entrypoint");
    let previous_extensions =
        ExtensionRegistry::discover(previous_root.path()).expect("previous registry");
    let previous = previous_extensions
        .select(["shared.id"])
        .expect("non-tool selection");
    let current_root = tempdir().expect("current extension root");
    let current_extensions =
        ExtensionRegistry::discover(current_root.path()).expect("current registry");
    let current = current_extensions
        .select_kind(ExtensionKind::Tool, std::iter::empty::<&str>())
        .expect("empty current selection");
    let mut tools = ToolRegistry::new();
    tools
        .register(MarkerTool::new("shared.id", json!({"adapter": "old"})))
        .expect("same-ID adapter");

    let error = reconcile_tool_selections(current_root.path(), &previous, &current, &mut tools)
        .expect_err("non-tool previous selection");

    assert!(matches!(
        error,
        ToolReconciliationError::WrongKind {
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

struct MarkerTool {
    id: ToolId,
    output: Value,
}

impl MarkerTool {
    fn new(id: &str, output: Value) -> Self {
        Self {
            id: ToolId::new(id).expect("tool ID"),
            output,
        }
    }
}

impl Tool for MarkerTool {
    fn id(&self) -> &ToolId {
        &self.id
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::Pure
    }

    fn invoke(&mut self, _input: &Value) -> Result<Value, ToolError> {
        Ok(self.output.clone())
    }
}
