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

## Ownership and trust boundary

The caller chooses the path. Successful parsing does not discover neighboring files, register a capability, grant permission, import code, execute an entrypoint, or persist state. A future loader must treat manifest declarations as untrusted metadata and apply its own identity, lifecycle, compatibility, permission, and isolation rules before activation.

## Non-goals

This boundary does not provide directory scanning, duplicate detection across manifests, registries, dependencies, enable/disable state, lifecycle hooks, activation, reload, filesystem watching, execution, tool authorization, sandboxing, persistence, or migration.
