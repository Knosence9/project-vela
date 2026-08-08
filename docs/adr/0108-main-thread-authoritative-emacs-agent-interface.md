# ADR-0108: Main-thread-authoritative Emacs agent interface

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1025](https://github.com/Knosence9/project-vela/issues/1025)
- **Related:** [ADR-0107](0107-bounded-python-adapter-execution.md)

## Context

Emacs is predominantly single-threaded. Buffer text, point, narrowing, overlays, windows, Org syntax state, Flymake diagnostics, and package-owned editor state are safest when observed or changed by short callbacks on Emacs's main thread. Running model inference, network requests, repository indexing, builds, or unbounded agent loops in those callbacks would freeze the user's editor and blur the authority boundary between observation and mutation.

Models also should not need to know package-specific Emacs Lisp or synthesize keystrokes to become effective Emacs users. Vela needs a stable semantic interface that can use native Org, project, diagnostics, compilation, and Magit facilities without granting arbitrary `eval` authority.

## Decision

Add `vela-agent-interface-mode`, a read-only `special-mode` dashboard backed by the same JSON-compatible typed responses intended for a future local JSON-RPC transport. Protocol version 1 initially exposes only:

- `capabilities.list`, which advertises read-only protocol operations and availability metadata for buffers, Org, project, Flymake diagnostics, compilation mode, and Magit, while identifying which facilities have exposed context sections; and
- `context.snapshot`, which returns only caller-selected bounded context sections. The initial sections are buffer metadata and native Org heading/source-block metadata.

Requests use an explicit operation name and explicit `include` vector. Unknown operations and context sections fail closed. There is no generic Emacs Lisp evaluation, arbitrary command execution, buffer mutation, Babel execution, tangling, exporting, shell invocation, or Magit mutation.

Capability availability uses constant-time callable-function bindings rather
than scanning `load-path`. The dispatcher rejects calls outside the editor-owner
thread captured at package load, and `context.snapshot` accepts at most the two
unique supported sections. It checks the vector length before copying it.
Top-level requests are bounded to eight unique string-keyed fields and validated
with a finite cursor walk, so cyclic, dotted, duplicate-keyed, and oversized
objects fail closed. Request keys and operation/section names have fixed
character limits.

Every context snapshot rejects source buffers larger than 1,048,576 characters
before line or Org traversal, source extraction, or hashing. This conservative
bound keeps synchronous work finite; later protocol versions may add narrower
field-specific bounds without weakening the fail-closed default. Dashboard JSON
is serialized in protocol alist order rather than hash-table iteration order.

Emacs remains authoritative for editor state. The interface reads that state synchronously in short main-thread callbacks and does not start long-running work. Future transport process filters may decode and enqueue bounded requests, but handling and every editor mutation must be scheduled back onto the main thread. Long-running reasoning, indexing, network access, builds, and computation belong in external cancellable workers. Their eventual results must re-enter Emacs through short callbacks with buffer identity and modification-tick preconditions.

## Concurrency model

1. An external worker performs expensive work outside Emacs.
2. A local transport receives a typed request and places it in a bounded queue.
3. A timer or process callback schedules a small unit of work on Emacs's main thread.
4. Read operations return bounded snapshots. Mutation operations, when later introduced, validate explicit buffer/file identity, expected modification tick, scope, and approval evidence before changing state.
5. Long operations return job handles instead of blocking Emacs. Cancellation and queue limits are part of the transport contract.

The first slice intentionally implements steps 3 and 4 only for direct, read-only in-process calls. It does not claim that a network or process transport exists.

## Alternatives considered

### Run the model or agent loop inside Emacs

Rejected because long inference, network, indexing, and subprocess waits can block the editor and make cancellation unreliable.

### Drive Emacs primarily with synthesized keys

Rejected because key sequences depend on focus, transient maps, user configuration, package versions, and hidden UI state. Typed semantic operations are more stable and auditable. Keyboard fallback may remain useful for unmodeled low-authority actions.

### Expose arbitrary `eval`

Rejected because it collapses observation, mutation, process, filesystem, and package authority into an untyped ambient capability.

### Parse Org only outside Emacs

Rejected as the primary authority because native Org APIs already own the editor's live syntax and package state. External parsers may support indexing, but delayed edits must still be validated against live Emacs state.

## Consequences

- The initial interface is useful for discovery and durable context inspection without making the UI wait for an agent.
- Org headings and source blocks are identified through native Org APIs; source text is represented by a SHA-256 digest rather than returned implicitly.
- The dashboard is human-inspectable but does not grant additional authority.
- The first slice is read-only and does not yet include JSON-RPC transport, request queues, edits, approvals, job handles, diagnostics payloads, Magit state, Babel execution, tangling, or export.
- Emacs is now a pinned development dependency, and byte compilation plus ERT tests are part of the repository quality gate.

## Verification

Batch byte compilation fails on warnings. ERT tests cover stable constant-time capability discovery and exposed-section metadata, explicit buffer context, native Org heading and source-block context, point/buffer preservation, worker-thread rejection, bounded unique sections before copying, cyclic and oversized request-object rejection, the oversized-buffer bound, deterministic interface JSON, interface rendering, malformed and unsupported-operation rejection, and unknown-section rejection. The complete repository quality gate must remain green.

## Revisit when

Revisit before adding transport, writable operations, Org Babel execution, tangling/export, Magit mutations, shell/compilation control, diagnostics payloads, asynchronous job handles, cancellation, or request queues.
