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

## Ownership and trust boundary

The caller chooses each manifest path or the one extension root. Successful standalone parsing does not consult configuration or inspect an entrypoint on the filesystem. Discovery validates that the entrypoint target is an extension-local regular file at discovery time, but successful parsing or discovery does not register a capability, grant permission, import code, execute an entrypoint, read target content, or persist state. Discovery is not an activation lease: a future loader must reopen targets relative to an anchored extension-directory descriptor, reject symlink or file-type escapes again, and apply its own identity, lifecycle, compatibility, permission, and isolation rules before activation.

## Non-goals

This boundary does not provide recursive or multi-root scanning, cross-root duplicate detection or precedence, mutable extension registries or selection toggles, dependencies, persisted enable/disable configuration, lifecycle hooks, skill or workflow activation, automatic refresh or reload, filesystem watching, replacement rollback history, config integration, tool authorization, persistence, or migration. A successful registry selection, kind-constrained selection, or kind projection records enablement intent only and is not activation or permission.
