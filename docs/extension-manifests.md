# Extension manifests

The `vela-extensions` crate owns the first non-executing contract for describing modular Vela capabilities. It parses and validates one caller-selected YAML manifest at a time; a valid manifest is metadata, not permission to load or run code. Shallow discovery additionally validates each package's declared entrypoint target without reading or activating it.

## Version 1 shape

```yaml
manifest_version: 1
id: local.search
kind: tool
entrypoint: local-search
description: Searches local project files
```

- `manifest_version` must be `1`.
- `id` is a stable caller-authored identifier containing at least one non-whitespace character. Surrounding whitespace is preserved.
- `kind` is exactly `tool`, `skill`, or `workflow`, matching the capability vocabulary in the project plans.
- `entrypoint` is a portable lexical path relative to the extension directory. It uses `/` separators and contains one or more non-blank normal components. Absolute paths, empty components, `.` or `..` components, ASCII control characters, Windows-reserved characters or device names, and components ending in a space or dot are rejected. These rules also reject Windows drive-prefixed and backslash-separated forms. The authored value is otherwise preserved.
- `description` is optional and preserved without reinterpretation.
- Unknown fields are rejected so misspellings and unsupported semantics do not silently enter the runtime contract.

`ExtensionManifest::load` reads at most 64 KiB plus one boundary byte before parsing. It accepts a manifest whose encoded size is exactly 64 KiB and returns a deterministic `TooLarge` error for the first byte beyond that limit. It also returns typed errors for unreadable files, malformed or structurally invalid YAML, unsupported versions, unsupported kinds, blank required strings, and invalid entrypoint paths. Read and parser failures remain available through `std::error::Error::source`.

## Shallow discovery

`discover_extensions(root)` inspects only `root/*/extension.yaml`. It ignores manifests directly in `root`, deeper nested manifests, unrelated files, and immediate child directories without a manifest. Candidate paths are sorted lexicographically before loading, so both successful results and the first validation failure are deterministic. Manifest symlinks are rejected without following their targets, including dangling symlinks.

Discovery currently requires a Unix target, where root, child-directory, manifest, and entrypoint traversal can remain anchored to directory descriptors and reject symlink traversal. After loading each manifest, discovery opens every intermediate entrypoint component relative to the already-open extension-directory descriptor, requires those components to be directories, and inspects the final component descriptor-relatively without following it. The final target must be a regular file; this check requires no read permission and reads no target content. Missing targets, symlinks at any component, and non-regular final targets fail discovery with a typed error containing the manifest path and authored entrypoint; the underlying I/O error remains available through `std::error::Error::source`. Other targets fail closed with a source-preserving `ReadRoot` error instead of reopening enumerated paths by name.

Each result contains the validated manifest and its source path. An unreadable root or directory-entry failure returns a typed, source-preserving root error with the failing directory path. An invalid candidate returns a typed, source-preserving manifest error with its path. Discovery fails as a whole rather than returning the valid prefix before an error.

One discovered root is an exact-ID namespace. After validating candidates in sorted path order, discovery rejects an ID that exactly equals one from an earlier manifest. The typed duplicate error contains the preserved ID, the first manifest path, and the duplicate manifest path; it has no underlying source. Comparison does not trim, case-fold, normalize, or otherwise reinterpret IDs, so IDs that differ only by case or surrounding whitespace remain distinct. The first sorted path is always the original and the next equal ID is always the reported duplicate.

## Registry snapshots

`ExtensionRegistry::discover(root)` owns an immutable, all-or-nothing snapshot of one successful discovery. `get(id)` resolves the exact caller-authored ID without trimming, normalization, case folding, authorization, or activation. `extensions()` borrows entries in lexicographic manifest-path order, preserving discovery order rather than sorting by ID. Empty roots produce empty snapshots, and discovery failures are returned unchanged without exposing a partial registry.

A registry never watches or automatically rescans its root. Filesystem changes do not alter an existing snapshot; a caller must explicitly construct a fresh registry to observe them. A refreshed snapshot repeats all descriptor-anchored package validation. Like discovery itself, a registry is metadata and not an activation lease.

`current.changes_from(previous)` compares two already-built snapshots without filesystem access or mutation. It returns changes in exact-ID order: `Added` borrows the current record, `Removed` borrows the previous record, and `Changed` borrows both records when the same exact ID has different manifest metadata or a different source path. Equal records are omitted. Comparison preserves authored ID case and whitespace and does not normalize, authorize, or activate either snapshot.

`registry.select(ids)` validates one caller-owned capability selection against an existing snapshot without filesystem access or mutation. Success returns an immutable `ExtensionSelection` that borrows the selected records from that snapshot, supports exact-ID lookup, and enumerates in exact-ID order regardless of request order; an empty request succeeds. Duplicate requested IDs and IDs absent from the snapshot fail the whole selection with typed, source-free errors. Selection preserves authored case and whitespace.

`selection.of_kind(kind)` returns another immutable selection containing only records of the requested validated `ExtensionKind`. The projection preserves exact-ID order, lookup, length, and emptiness semantics, borrows the same registry records, and returns an empty selection when that kind is absent. It lets a future lifecycle layer route enablement intent toward the separate tool, skill, and workflow registries described by the architecture without opening entrypoints or treating metadata as activated.

`registry.select_kind(kind, ids)` is the fail-closed counterpart for caller intent that must target exactly one capability kind. It preserves the generic selection operation's borrowed records, exact-ID ordering, duplicate and absent-ID errors, and empty-selection success. An existing ID of another validated kind fails the whole operation with a typed, source-free error containing the exact ID and expected and actual kinds; it is not silently omitted. Duplicate errors take precedence over lookup, then requested IDs are checked in exact-ID order so the first absent or mismatched ID is deterministic.

Discovery is the installed metadata catalog; a selection records only in-memory enablement intent. To represent disabled intent for a subsequent operation, construct a new selection that omits the ID. A selection cannot outlive or rebind itself to its originating registry, and it does not persist configuration, authorize, load, execute, or activate a capability. Future activation must separately reopen an entrypoint through the descriptor-anchored boundary, and each future tool invocation must still receive caller-owned permission.

The accepted first execution boundary is specified by [ADR-0003](adr/0003-tools-only-wasm-component-boundary.md). It limits activation to kind-constrained tool selections and no-import WebAssembly components. Selection remains inert until a caller explicitly invokes the separate activation operation below.

## Tool artifact preparation

`prepare_tool_artifacts(root, selection)` is the non-executing bridge between a kind-constrained selection and the later component compiler. It reopens the caller-supplied original root without following a final-component symlink, requires that root and every selected package still have their discovery-time filesystem identities, reloads each manifest through its anchored package descriptor, and requires the manifest and expected source location to match the selected record exactly. Lexically different root paths containing `.` or `..`, and aliases in parent components, are accepted when they reopen the same root identity; a final-component root symlink remains rejected. It then reopens every entrypoint component relative to that same package descriptor, rejecting symlinks and non-regular targets.

Each entrypoint read accepts at most 16 MiB (`MAX_ENTRYPOINT_BYTES`) and probes one additional boundary byte before returning `EntrypointTooLarge`. Success returns owned `PreparedToolArtifact` values in exact-ID order, preserving each exact ID and its bytes. The operation is all-or-nothing: a changed or moved package, changed manifest, mismatched root, wrong capability kind, unsafe target, read failure, or oversized target returns one deterministic `ExtensionPreparationError` and no artifact prefix. Filesystem and manifest failures remain available through `std::error::Error::source`.

Preparation does not compile or validate a WebAssembly component, mutate a registry, authorize a tool, persist state, or execute guest code. Those remain later activation and invocation boundaries.

## Skill instruction preparation

The accepted first skill boundary is specified by [ADR-0004](adr/0004-inert-utf8-skill-preparation.md). `prepare_skill_artifacts(root, selection)` accepts only selected version-one `Skill` records and rejects the lexicographically first other kind before filesystem access. An empty selection returns an empty result without opening the root.

For a non-empty selection, preparation reuses the descriptor-anchored root, package, manifest, and entrypoint revalidation boundary described above. Each entrypoint read accepts at most 1 MiB (`MAX_SKILL_INSTRUCTION_BYTES`) plus one boundary probe byte and must be valid UTF-8. Success returns immutable owned exact IDs and exact authored instruction text in exact-ID order. Any identity, manifest, entrypoint, size, encoding, or kind failure returns no artifact prefix; sourced failures remain available through `std::error::Error::source`.

Prepared skill instructions are inert data. Preparation does not parse Markdown or front matter, register or activate a skill, compose a provider prompt, execute content, grant tool permission, persist enablement, watch files, or activate workflows. Registration and later prompt composition follow the separate contract in [ADR-0005](adr/0005-caller-owned-skill-registration-and-prompt-composition.md).

## Atomic inert skill registration

`register_skill_selection(root, selection, registry)` first rejects the lexicographically first selected non-skill before filesystem access, then reuses the complete descriptor-anchored skill preparation boundary and calls the caller-owned process-local `SkillRegistry` once. The registry preflights exact-ID collisions against existing instructions and within the batch before inserting anything. Any kind, preparation, encoding, or collision failure leaves every registered skill unchanged. An empty selection performs no filesystem access or mutation.

Success preserves exact authored UTF-8 and exposes registered skills in deterministic exact-ID order. Debug output records instruction length rather than instruction bodies. Registration expresses availability only: it does not select a skill for a request, call a provider, compose a prompt, persist enablement, grant tool authority, parse instructions, replace registrations, or activate workflows. ADR-0005 requires a later explicit per-request selection and provider-neutral composition boundary before any registered skill may influence a model turn.

## Declarative workflow definition preparation

The workflow preparation contract is specified by [ADR-0006](adr/0006-inert-declarative-workflow-definitions.md) and extended with inert phase skill bindings by [ADR-0018](adr/0018-inert-workflow-phase-skill-bindings.md). `prepare_workflow_definitions(root, selection)` accepts only selected `Workflow` records, rejects the lexicographically first other kind before filesystem access, and treats an empty selection as a filesystem-free success. A non-empty selection reuses descriptor-anchored root, package, manifest, and entrypoint revalidation before reading at most 1 MiB plus one probe byte.

Each entrypoint must be UTF-8 strict YAML. Version one retains the original topology-only shape and exposes empty phase skill bindings. Version two additionally permits an optional authored-order `skills` sequence on non-terminal phases. Skill IDs must be exact non-blank values and unique within the phase; terminal phases reject a `skills` field. Unknown fields, blank or duplicate phase IDs, absent starts or transition targets, blank present gate IDs, terminal phases with transitions, non-terminal phases without transitions, and phases unreachable from the start are rejected. A successful immutable definition preserves exact IDs, phase order, skill order, transition order, terminal markers, targets, and optional gates.

Prepared workflow topology and phase skill bindings are inert. The operation does not resolve a skill registry, require a bound skill to exist, select or compose instructions, register, activate, execute, schedule, pause, resume, persist, retry, choose a transition, evaluate a gate, bind tools/agents/humans, mutate a prompt, or grant permission. Cycles are structurally valid when reachable; this boundary validates topology and procedure intent but does not prove eventual termination.

## Atomic inert workflow registration

The accepted registration boundary is specified by [ADR-0007](adr/0007-caller-owned-inert-workflow-registration.md). The kernel `WorkflowRegistry` is a caller-owned, process-local exact-ID directory of immutable owned workflow topology. Atomic batch registration rejects the lexicographically first collision against either the existing registry or the batch without mutation. Exact lookup preserves identity, registry enumeration uses ascending exact-ID order, and phases and transitions retain authored order internally.

`register_workflow_selection(root, selection, registry)` rejects the lexicographically first selected non-workflow before filesystem access, reuses the complete descriptor-anchored preparation and graph-validation boundary, converts every successful definition to kernel topology, and mutates the registry once. Preparation and registry failures reject the whole batch and preserve their underlying sources. An empty selection is a filesystem-free no-op.

Registration expresses availability only. It does not select, execute, schedule, persist, resolve gates, choose transitions, bind phase actions, compose provider requests, grant permissions, replace or remove definitions, watch files, or hot reload.

## Explicit current-phase skill resolution

The caller-owned resolution boundary is specified by [ADR-0019](adr/0019-explicit-current-phase-skill-resolution.md). `RegisteredWorkflowPhase::resolve_skills(registry)` explicitly resolves one caller-chosen borrowed phase against one borrowed process-local `SkillRegistry`. Exact inert binding strings are validated as `SkillId` values before the complete batch reuses `SkillRegistry::select`. Success returns borrowed registered instruction blocks in deterministic ascending exact-ID order; authored binding order remains unchanged in workflow topology and does not imply precedence. Empty bindings select nothing, including when unrelated skills are registered.

Malformed direct terminal bindings fail with `WorkflowPhaseSkillResolutionError::TerminalHasBindings`, and malformed direct IDs fail with a source-preserving `WorkflowPhaseSkillResolutionError::InvalidId`. Duplicate or missing registrations preserve the existing typed `SkillSelectionError` through `WorkflowPhaseSkillResolutionError::Selection`, and no failure returns a partial selection. Missing process-local registrations do not prevent workflow preparation, registration, start, replay, discovery, or advancement; they affect only this explicit resolution attempt.

Resolution is read-only and provider-neutral. It does not invoke a provider, compose or mutate a request by itself, persist selection evidence, infer workflow lifecycle eligibility, advance a cursor or durable run, evaluate a gate, schedule work, grant tool permission, or infer that selected instructions are safe.

## Explicit workflow-phase provider composition

The tool-free composition bridge is specified by [ADR-0020](adr/0020-explicit-workflow-phase-provider-composition.md). `AssistantRuntime::execute_workflow_phase_turn` requires the caller to supply one exact borrowed phase together with the session, human content, system policy, developer policy, and caller-owned skill registry. It resolves the phase through the boundary above before any transcript or provider side effect, then reuses the existing composed-turn authority structure. The provider receives system policy, developer policy, phase-bound registered skills in deterministic ascending exact-ID order, and the durable transcript as distinct fields. Registered but unbound skills are excluded.

Resolution failures surface as typed `RuntimeError::WorkflowPhaseSkills` errors before the human turn is persisted or the provider is called. After resolution succeeds, existing composed-turn session and provider durability semantics remain unchanged. The operation does not infer a current phase, load or mutate a workflow run, choose a transition, evaluate a gate, schedule work, persist selection evidence, grant tool permission, or invoke a tool; the caller remains responsible for explicitly choosing the phase.

## Explicit workflow-phase task Attempt evidence

The task-evidence bridge is specified by [ADR-0021](adr/0021-workflow-phase-task-attempt-evidence.md). `AssistantRuntime::execute_workflow_phase_task_turn` requires an exact active task already associated with a writable session, a fresh Attempt observation ID, and the same explicit phase, registry, human content, and policy inputs as the session-only operation. Before transcript or provider effects, it validates the task association and lifecycle, Attempt identity, session writability, and phase bindings. A successful response is committed first as the assistant transcript turn and then as the task's exact Attempt text.

Provider failure preserves the committed human turn and appends no Attempt; later persistence failures preserve earlier commits. The Attempt is response evidence only: the operation does not accept or infer a workflow-run ID, prove phase provenance, persist phase or skill-selection identity, synchronize task and workflow lifecycles, infer success, complete either aggregate, choose or apply a transition, schedule work, grant tools, or invoke tools.

## Tool component compilation

`compile_tool_components(artifacts)` is the inert compiler boundary for prepared tools. It creates a Wasmtime engine with the Component Model explicitly enabled and compiles binary component bytes without Wasmtime's text-format support. Every component must have no top-level imports and exactly one top-level export: the synchronous function `invoke(input: string) -> result<string, string>` specified structurally by `vela:extension/tool@0.1.0`. Core modules, malformed bytes, imports, missing or additional exports, and incompatible parameter or result types fail closed.

Success returns owned `CompiledToolComponent` values in artifact input order, preserving exact IDs. Compilation is all-or-nothing and creates no store or instance, so it cannot call a guest export. Engine and binary-compilation failures preserve the Wasmtime error source; structural ABI failures preserve a typed `ToolComponentAbiError`; artifact-specific failures expose the exact ID. Each compiled component remains inert until explicitly consumed by the adapter described below; adapter construction is inert as well.

Compilation itself does not instantiate components, register adapters, mutate a tool registry, authorize invocations, validate JSON payloads, apply per-invocation limits, persist state, or expose WASI or other host imports. The adapter below owns invocation behavior; the activation operation owns registration.

## Tool component invocation

`ComponentTool::new(compiled)` adapts one inert `CompiledToolComponent` to the kernel `Tool` contract. Construction preserves the exact manifest ID as `ToolId`, classifies version-one components as `ToolEffect::Pure`, and creates no store or instance. The `id()` and `effect()` accessors likewise call no guest code. Callers retain the existing kernel authorization boundary and must invoke an adapter only through that boundary.

Every authorized `invoke` serializes the caller-owned `serde_json::Value`, creates a fresh Wasmtime store and no-import component instance, calls the exact synchronous ABI once, and discards all guest state afterward. A successful guest string must parse as exactly one JSON value. Guest `err(string)` diagnostics, malformed successful JSON, instantiation failures, traps, fuel exhaustion, epoch interruption, and resource-limit failures become sourced `ToolError` failures without retry or persistence. No WASI or other host imports are linked.

The default policy permits at most 16 MiB per linear memory, 10,000 elements per table, 100 core instances, 10 linear memories, 10 tables, and 10,000,000 fuel units per invocation. A shared 10 ms engine ticker enforces a one-second epoch deadline without allowing one invocation's private timer to shorten another invocation's configured deadline. `ToolExecutionLimits` makes this implementation policy explicit and replaceable at adapter construction; these values are not part of `vela:extension/tool@0.1.0`.

Invocation does not register adapters, activate a selected batch, authorize itself, retry, persist guest diagnostics or outputs, reuse a store or instance, expose broader effects, or grant host capabilities.

## Atomic tool activation

`activate_tool_selection(root, selection, registry)` composes preparation, compilation, inert adapter construction with `ToolExecutionLimits::default()`, and registration into the explicit root-to-registry boundary. `activate_tool_selection_with_limits(root, selection, registry, limits)` provides the same boundary with one caller-selected resource policy applied uniformly to the full batch. Restrictive limits do not instantiate or reject an otherwise valid guest during activation; they take effect only in each fresh store created after a later invocation is authorized.

Both operations first reject the lexicographically first selected non-tool record with a source-free `ToolActivationError::WrongKind`, before opening the supplied root or performing any other preparation. They then complete every pre-registration stage for the whole exact-ID selection before mutating the caller-owned `ToolRegistry`. The registry's homogeneous batch operation preflights collisions against existing adapters and within the batch before inserting anything, so every kind, preparation, compilation, construction, or duplicate-ID failure leaves existing registry metadata and adapters unchanged. An empty selection succeeds without filesystem access or registry mutation under either policy.

Activation errors identify the failed stage and preserve its typed source. Successful metadata is ordered by exact `ToolId`, every version-one adapter remains `Pure`, and neither compilation, adapter construction, metadata inspection, nor registration creates a store or instance. Guest code can run only through a later registry invocation after caller-owned authorization. Activation does not replace or remove adapters, persist configuration, enable hot reload, activate skills or workflows, expose host imports, or grant broader effects.

## Atomic tool deactivation

`deactivate_tool_selection(selection, registry)` atomically unregisters the exact adapters named by one tool selection. It first rejects the lexicographically first selected non-tool record with a source-free `ToolDeactivationError::WrongKind`, then preflights the complete batch through the kernel registry before mutation. Any selected ID absent from the active registry returns `ToolDeactivationError::Registry` with the typed, source-free `ToolRegistryRemovalError`; every failure leaves every adapter unchanged. The underlying kernel batch operation also rejects duplicate requested IDs before lookup, though an `ExtensionSelection` cannot contain duplicates. Empty deactivation is a no-op. Success removes only the selected adapters and preserves unrelated registry metadata in exact-ID order.

Deactivation uses only the immutable selected records. It does not reopen the extension root, inspect a manifest or entrypoint, instantiate or invoke guest code, or consult authorization. A removed adapter is no longer discoverable or invocable, while the extension registry and selection remain inert metadata. Reactivation must repeat the existing descriptor-anchored preparation and atomic activation boundary. The operation is synchronous and caller-owned; it does not interrupt an invocation already in flight, persist enablement, replace adapters, watch the filesystem, or enable hot reload.

## Atomic active-tool replacement

`replace_tool_selection(root, selection, registry)` explicitly refreshes one selected active tool batch with default limits. `replace_tool_selection_with_limits(root, selection, registry, limits)` applies one caller-selected resource policy uniformly to the replacement batch. Both operations repeat descriptor-anchored preparation, exact ABI compilation, and inert adapter construction for the complete selection before calling `ToolRegistry::replace_all`. Restrictive limits do not instantiate a component during replacement; they apply only to later authorized invocations.

Replacement first rejects the lexicographically first selected non-tool record with a source-free `ToolReplacementError::WrongKind`, before opening the supplied root or performing any other preparation. Every selected exact ID must then already be active. A kind, preparation, compilation, construction, duplicate-ID, or missing-active-ID failure preserves the complete old invocable batch and every unrelated adapter. An empty selection succeeds without filesystem or registry access. Success swaps exactly the selected adapters; future invocations still require a fresh caller-owned authorization decision and fresh isolated store.

This operation is an explicit synchronous caller-driven refresh. It does not watch the filesystem, automatically react to a registry snapshot change, persist activation or enablement, retain rollback history, migrate guest state, cancel an invocation already in flight, or activate skills and workflows. Rust mutable borrowing prevents replacement through the same registry during an invocation.

## Atomic active-tool reconciliation

`reconcile_tool_selections(root, previous, current, registry)` transitions the adapters owned by one previous tool selection to one current tool selection using default limits. `reconcile_tool_selections_with_limits(root, previous, current, registry, limits)` applies one caller-selected resource policy uniformly to every current adapter. Both selections must contain only validated tools. The complete current selection is descriptor-revalidated against the supplied current root, compiled, and adapted inertly before registry mutation.

The kernel commit removes previous-only IDs, replaces IDs shared by both selections, and registers current-only IDs as one atomic operation. Every previous ID must be active, while a current-only ID must not collide with an unrelated active adapter. Duplicate previous IDs, duplicate current adapters, missing previous IDs, and unrelated collisions fail in that deterministic order with the lexicographically first exact ID. Any validation, preparation, compilation, construction, or registry-preflight failure preserves the complete old selected batch and every unrelated adapter. Empty-to-empty reconciliation is a no-op without filesystem access.

Selections are explicit caller-owned lifecycle evidence: reconciliation never infers adapter ownership from registry metadata. It rebuilds even unchanged current IDs so the current root and artifacts receive the same revalidation as additions. The operation does not watch files, react automatically to snapshot differences, persist enablement, retain rollback history, interrupt an invocation already in flight, invoke guest code during the transition, activate non-tool kinds, or bypass later per-invocation authorization.

## Developer CLI inspection

`vela-dev extension inspect <ROOT>` exposes the immutable discovery catalog without adding another parsing or filesystem boundary. It delegates to `ExtensionRegistry::discover`, then prints one tab-separated record per extension in deterministic manifest-path order: debug-escaped exact authored ID, validated lowercase kind, debug-escaped authored entrypoint, and debug-escaped root-relative manifest path. Quoted escaping keeps untrusted delimiters, newlines, terminal controls, and path bytes inside one field. A final line reports the extension count; an empty valid root reports zero.

Discovery remains all-or-nothing. An unreadable or invalid root exits non-zero with one root-scoped, debug-escaped diagnostic and emits no partial records or success summary. Diagnostic escaping prevents untrusted discovery paths or metadata from adding lines or terminal controls. The command is read-only: it does not recursively or automatically refresh the root, select IDs, activate or compile entrypoints, instantiate or invoke guests, authorize tools, mutate registries, infer configuration, or persist enablement.

## Developer CLI invocation

`vela-dev extension invoke <ROOT> <EXACT_ID> <INPUT_JSON>` connects one explicit developer request to the existing library boundaries without adding another loader or executor. It parses the input as exactly one JSON value before touching the extension root, discovers the complete root, selects exactly one validated `Tool` ID, activates only that selection with default limits into a fresh process-local registry, and invokes it through the kernel permission protocol. The CLI-owned authorizer permits only the selected exact ID, only its version-one `Pure` effect, and only once.

Success prints exactly one compact JSON value followed by a newline. Malformed `INPUT_JSON` and discovery, selection, activation, authorization, guest, trap, resource-limit, or malformed-output failures print no partial stdout and emit one debug-escaped single-line diagnostic. The explicit invocation is not a reusable grant: the command does not persist activation, permission, input, or output; retry; watch or refresh the root; expose host imports; or activate skills and workflows.

## Ownership and trust boundary

The caller chooses each manifest path or the one extension root. Successful standalone parsing does not consult configuration or inspect an entrypoint on the filesystem. Discovery validates that the entrypoint target is an extension-local regular file at discovery time, but successful parsing or discovery does not register a capability, grant permission, import code, execute an entrypoint, read target content, or persist state. Discovery is not an activation lease: a future loader must reopen targets relative to an anchored extension-directory descriptor, reject symlink or file-type escapes again, and apply its own identity, lifecycle, compatibility, permission, and isolation rules before activation.

## Non-goals

This boundary does not provide recursive or multi-root scanning, cross-root duplicate detection or precedence, mutable extension registries or selection toggles, dependencies, persisted enable/disable configuration, lifecycle hooks, skill or workflow activation, automatic refresh or reload, filesystem watching, replacement rollback history, config integration, tool authorization, persistence, or migration. A successful registry selection, kind-constrained selection, or kind projection records enablement intent only and is not activation or permission.
