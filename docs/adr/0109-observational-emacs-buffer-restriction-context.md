# ADR-0109: Observational Emacs buffer restriction context

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1027](https://github.com/Knosence9/project-vela/issues/1027)
- **Related:** [ADR-0108](0108-main-thread-authoritative-emacs-agent-interface.md)

## Context

ADR-0108 makes live Emacs state authoritative and permits short, bounded context snapshots on the editor-owner thread. Protocol version 1 reports point, line, column, active-region bounds, and native Org context, but does not reveal whether Emacs has narrowed the source buffer. A caller can therefore mistake positions observed inside an accessible restriction for positions observed against the complete buffer.

The interface must resolve that ambiguity without widening the buffer, returning text, adding filesystem discovery, or creating delayed edit authority.

## Decision

Advance the Emacs agent interface protocol to version 2. Append one ordered `restriction` object to the existing `buffer` context:

- `start` is the current Emacs-native, 1-based `point-min`;
- `end` is the current exclusive `point-max`; and
- `narrowed` is the JSON boolean representation of `buffer-narrowed-p`.

Extraction remains a synchronous, fixed-width read on the editor-owner thread. It does not widen the buffer. The returned bounds are observational snapshot metadata only: they expose no inaccessible or accessible text, grant no mutation authority, and are not modification-tick preconditions for a later edit.

No new operation, context section, capability, request width, transport, queue, package discovery, or external worker authority is introduced.

## Alternatives considered

### Keep protocol version 1

Rejected because the exact response shape changes. A version increment lets consumers fail closed rather than silently treating the extended object as the original contract.

### Widen while collecting global positions

Rejected because widening changes the live accessibility boundary during the callback and conceals the state that callers need to understand.

### Return accessible text with the bounds

Rejected because text access is a separate disclosure and allocation boundary. Restriction metadata is sufficient to disambiguate the existing positional snapshot.

### Expose project or diagnostics context first

Rejected for this slice. Project discovery can invoke configurable filesystem or VC hooks, while diagnostics require separate count, message, position, and stale-state bounds. Restriction metadata closes an existing correctness gap with constant work and no new package authority.

## Consequences

- Callers can distinguish complete-buffer and narrowed positional snapshots.
- Emacs-native 1-based, exclusive-end position semantics are explicit.
- Protocol consumers must accept version 2 before relying on the extended buffer object.
- Snapshot bounds can become stale immediately after the callback and cannot authorize delayed edits.
- Diagnostics payloads, project context, transport, jobs, cancellation, and mutation remain deferred.

## Verification

RED→GREEN ERT coverage pins the version increment, exact unnarrowed object shape, narrowed bounds, and preservation of restriction, point, mark, match data, full text, modified state and tick, and undo state. Batch byte compilation treats warnings as errors, and the complete repository quality gate must remain green.

## Revisit when

Revisit before returning buffer text, adding modification-tick edit preconditions, exposing diagnostics or project context, or introducing any asynchronous transport or mutation operation.
