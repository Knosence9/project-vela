# ADR-0110: Observational Emacs buffer text revision

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1029](https://github.com/Knosence9/project-vela/issues/1029)
- **Related:** [ADR-0108](0108-main-thread-authoritative-emacs-agent-interface.md), [ADR-0109](0109-observational-emacs-buffer-restriction-context.md)

## Context

ADR-0108 requires future delayed edits to return to Emacs with live buffer identity and modification-tick preconditions. Protocol version 2 identifies the observed buffer by name and optional file and reports its accessible restriction, but external work cannot determine whether the observed characters changed before its result returns.

Adding transport or edits now would prematurely combine queueing, cancellation, targeting, identity, approval, scope, and mutation policy. The smallest useful slice is read-only revision evidence from the editor that owns the live text.

## Decision

Advance the Emacs agent interface to protocol version 3. Append `text_revision` immediately before `restriction` in the ordered `buffer` context. Its value is the current non-negative integer returned by `buffer-chars-modified-tick`.

The revision is opaque equality evidence. A caller may compare two revisions for equality to detect stale character observations. It must not perform arithmetic on revisions, infer how many edits occurred, assume revisions are globally unique, or use a matching revision as sufficient edit authority. Future delayed mutation must also validate live buffer identity, accessibility, operation scope, and Vela approval evidence on Emacs's owner thread.

Snapshotting remains observational. It does not alter text or text properties, point, mark, match data, narrowing, modified state or tick, or undo state. No operation, context section, include width, transport, targeting mechanism, text access, filesystem discovery, or mutation capability is added.

## Alternatives considered

### Return `buffer-modified-p` only

Rejected because the modified flag describes divergence from the visited or saved state, not whether characters changed since an external observation.

### Hash the entire accessible buffer

Rejected because synchronous full-text hashing adds unnecessary owner-thread work and would make narrowing affect the revision evidence. Emacs already owns a constant-time character modification tick.

### Add a writable operation with the tick

Rejected because a revision alone does not solve transport authentication, buffer targeting and identity, approval evidence, edit scope, queue bounds, or cancellation.

## Consequences

- External work can preserve a bounded native staleness token alongside a read-only snapshot.
- The token deliberately detects character changes, not every property-only change or unrelated editor-state transition.
- Protocol consumers must update exact response handling for version 3 and the appended ordered field.
- Mutation authority remains absent.

## Verification

ERT pins protocol version and deterministic field order, verifies a non-negative revision, verifies stable repeated snapshots, verifies a changed revision after character insertion, and preserves editor state including text properties. The full repository quality gate must remain green.
