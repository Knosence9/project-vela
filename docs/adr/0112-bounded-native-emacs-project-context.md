# ADR-0112: Bounded native Emacs project context

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1035](https://github.com/Knosence9/project-vela/issues/1035)
- **Related:** [ADR-0108](0108-main-thread-authoritative-emacs-agent-interface.md), [ADR-0111](0111-process-local-live-emacs-buffer-identity.md)

## Context

ADR-0108 catalogs native `project.el` as an intended semantic facility, but protocol version 4 exposes no project context. External work can observe a buffer and Org location yet cannot bind that observation to the project Emacs currently recognizes. Guessing from `buffer-file-name` would duplicate project-backend policy, fail for non-file buffers, and encourage filesystem discovery outside the editor authority boundary.

Project file enumeration, indexing, and VCS inspection can be expensive. The smallest responsible slice is therefore only the native project root that Emacs resolves for the current buffer, captured through the existing shape- and output-bounded owner-thread request. Native project backends are trusted editor extensions and may have backend-specific lookup latency; this protocol does not claim to impose a synchronous time limit on them.

## Decision

Advance the Emacs agent interface to protocol version 5. Expose `project` as a third optional `context.snapshot` section and identify it in capability metadata. The section is JSON null when `project-current` finds no project. Otherwise it contains one ordered field, `root`, with the bounded absolute directory string returned by `project-root`.

Resolution calls `project-current` without prompting and `project-root` only for a resolved project. The root must be a string, absolute, and no longer than the existing 8,192-character editor-metadata bound; invalid values fail closed. Native lookup runs only through the existing editor-owner-thread dispatcher, and project extraction preserves caller match data. The complete snapshot retains the existing 1,048,576-character source-buffer cap.

The `include` vector now accepts at most the three unique sections `buffer`, `org`, and `project`. Cardinality is checked before copying. Duplicate detection is generalized across all three allowed positions.

The root is observational metadata only. It does not authorize filesystem access, project switching, indexing, process execution, VCS operations, or mutation. No project files are enumerated or read.

## Alternatives considered

### Infer a project from the visited file

Rejected because native project backends own project recognition, and buffers need not visit files.

### Return project files or backend-specific metadata

Rejected because enumeration may be expensive or remote, increases disclosure, and is unnecessary for establishing bounded project identity.

### Resolve the project outside Emacs

Rejected because external discovery can disagree with live native backend configuration and bypasses the owner-thread authority boundary.

## Consequences

- Agent work can correlate a live snapshot with the project Emacs recognizes without guessing from paths.
- Buffers outside a project have an explicit null representation.
- A project root may disclose an absolute path already known to Emacs; callers must still obtain separate authority for any filesystem effect.
- Project discovery remains synchronous, native, and non-prompting. Request shape and returned metadata are bounded, while native backend latency remains subject to the trusted editor configuration; indexing and transport remain absent.

## Verification

ERT pins protocol version and capability metadata; covers resolved and missing projects, oversized roots, three-section uniqueness and pre-copy cardinality; and verifies project snapshots preserve point, mark, narrowing, text, modification state, revision, undo state, and match data. Existing byte-compilation, interface, owner-thread, request-bound, response-bound, and complete repository quality gates remain green.
