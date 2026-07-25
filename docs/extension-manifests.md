# Extension manifests

The `vela-extensions` crate owns the first non-executing contract for describing modular Vela capabilities. It parses and validates one caller-selected YAML manifest at a time; a valid manifest is metadata, not permission to load or run code.

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
- `entrypoint` is an opaque string containing at least one non-whitespace character. Parsing does not resolve or execute it.
- `description` is optional and preserved without reinterpretation.
- Unknown fields are rejected so misspellings and unsupported semantics do not silently enter the runtime contract.

`ExtensionManifest::load` reads at most 64 KiB plus one boundary byte before parsing. It accepts a manifest whose encoded size is exactly 64 KiB and returns a deterministic `TooLarge` error for the first byte beyond that limit. It also returns typed errors for unreadable files, malformed or structurally invalid YAML, unsupported versions, unsupported kinds, and blank required strings. Read and parser failures remain available through `std::error::Error::source`.

## Shallow discovery

`discover_extensions(root)` inspects only `root/*/extension.yaml`. It ignores manifests directly in `root`, deeper nested manifests, unrelated files, and immediate child directories without a manifest. Candidate paths are sorted lexicographically before loading, so both successful results and the first validation failure are deterministic. Manifest symlinks are rejected without following their targets, including dangling symlinks.

Discovery currently requires a Unix target, where root, child-directory, and manifest opens can remain anchored to directory descriptors and reject symlink traversal. Other targets fail closed with a source-preserving `ReadRoot` error instead of reopening enumerated paths by name.

Each result contains the validated manifest and its source path. An unreadable root or directory-entry failure returns a typed, source-preserving root error. An invalid candidate returns a typed, source-preserving manifest error with its path. Discovery fails as a whole rather than returning the valid prefix before an error.

## Ownership and trust boundary

The caller chooses each manifest path or the one extension root. Successful parsing or discovery does not consult configuration, register a capability, grant permission, import code, execute an entrypoint, or persist state. A future loader must treat manifest declarations as untrusted metadata and apply its own identity, lifecycle, compatibility, permission, and isolation rules before activation.

## Non-goals

This boundary does not provide recursive or multi-root scanning, duplicate detection across manifests, registries, dependencies, enable/disable state, lifecycle hooks, activation, reload, filesystem watching, config integration, execution, tool authorization, sandboxing, persistence, or migration.
