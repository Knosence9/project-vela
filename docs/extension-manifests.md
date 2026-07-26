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

Discovery is the installed metadata catalog; a selection records only in-memory enablement intent. To represent disabled intent for a subsequent operation, construct a new selection that omits the ID. A selection cannot outlive or rebind itself to its originating registry, and it does not persist configuration, authorize, load, execute, or activate a capability. Future activation must separately reopen an entrypoint through the descriptor-anchored boundary, and each future tool invocation must still receive caller-owned permission.

## Ownership and trust boundary

The caller chooses each manifest path or the one extension root. Successful standalone parsing does not consult configuration or inspect an entrypoint on the filesystem. Discovery validates that the entrypoint target is an extension-local regular file at discovery time, but successful parsing or discovery does not register a capability, grant permission, import code, execute an entrypoint, read target content, or persist state. Discovery is not an activation lease: a future loader must reopen targets relative to an anchored extension-directory descriptor, reject symlink or file-type escapes again, and apply its own identity, lifecycle, compatibility, permission, and isolation rules before activation.

## Non-goals

This boundary does not provide recursive or multi-root scanning, cross-root duplicate detection or precedence, mutable registries or selection toggles, dependencies, persisted enable/disable configuration, lifecycle hooks, activation, automatic refresh or reload, filesystem watching, config integration, execution, tool authorization, sandboxing, persistence, or migration. A successful registry selection records enablement intent only and is not activation or permission.
