# ADR-0114: Bounded native Emacs compilation context

- **Status:** accepted
- **Date:** 2026-08-09
- **Decision and execution issue:** [#1039](https://github.com/Knosence9/project-vela/issues/1039)
- **Related:** [ADR-0108](0108-main-thread-authoritative-emacs-agent-interface.md), [ADR-0113](0113-bounded-current-line-flymake-diagnostics.md)

## Context

ADR-0108 catalogs native compilation mode as an intended model-neutral facility but exposes no structured compilation context. An agent viewing a native compilation buffer therefore cannot distinguish active work from an inactive buffer or observe the diagnostic counters already maintained by compilation mode without package-specific Emacs Lisp or UI scraping.

Selecting `compilation-last-buffer`, traversing `compilation-in-progress`, reading command or environment state, forcing output parsing, or inferring completion from presentation text would cross unrelated global, process, filesystem, or execution boundaries. Native compilation sentinels can also delete a finished process, so process absence is not durable success or failure evidence.

## Decision

Advance the Emacs agent protocol to version 7. Add `compilation` as a fifth optional `context.snapshot` section and identify it on the existing compilation capability.

The section examines only the current buffer. It returns JSON null unless `compilation-buffer-p` is already callable and recognizes that buffer; snapshotting does not load or search for optional libraries and never chooses a global or last compilation buffer. A recognized buffer returns these fields in deterministic order:

- `process_active`: true only when `get-buffer-process` returns an associated process that `process-live-p` considers live;
- `errors`: the current buffer-local `compilation-num-errors-found` value;
- `warnings`: the current buffer-local `compilation-num-warnings-found` value; and
- `infos`: the current buffer-local `compilation-num-infos-found` value.

Each counter must have a buffer-local binding and be a non-negative integer no greater than 1,048,576. Missing, inherited, malformed, negative, or oversized state fails closed. The existing 1,048,576-character source-buffer limit, deterministic response encoder, and editor-owner-thread boundary remain in force. The complete legal five-section response is admitted by the finite response-node bound.

These values are observational progress metadata. Counters reflect only findings native compilation mode has already recognized and may be incomplete or stale immediately. `process_active: false` does not prove success, failure, completion, cancellation, or that a process ever existed.

Snapshotting preserves point, mark, narrowing, text and text properties, modification state and tick, undo state, and match data. It does not start, restart, stop, signal, kill, or wait for a process; expose commands, environment, directories, output, process objects, filters, or sentinels; force parsing or fontification; navigate errors; visit or read files; or add transport, queues, mutation, approvals, or Magit behavior.

## Alternatives considered

### Select the last or globally active compilation

Rejected because global compilation state may belong to an unrelated project or buffer, has no bounded caller-selected scope, and can leak cross-context process metadata.

### Report exit status or successful completion

Rejected because native finished processes may already be deleted and inactive state cannot distinguish completion outcomes. Parsing mode-line or inserted footer text would make presentation state an unreliable protocol authority.

### Return command, directory, or output text

Rejected because those values can contain secrets, disclose filesystem state, grow with process output, and require separate permission and paging contracts.

### Force native parsing before reading counters

Rejected because parsing may mutate package state, perform work proportional to output, and violate the short observational owner-thread boundary.

## Consequences

- Agents can inspect bounded current-buffer compilation progress without package-specific evaluation or UI scraping.
- The section remains useful for native and derived compilation facilities while making no claim that the buffer represents a build.
- A live associated process and recognized counters are visible, but durable lifecycle outcome and output remain deliberately unavailable.
- Context requests may include each of `buffer`, `org`, `project`, `diagnostics`, and `compilation` at most once.

## Verification

RED→GREEN ERT coverage pins protocol version 7, capability metadata, recognized and null section shapes, active and inactive process observations, exact counter values, malformed and nonlocal counter rejection, five-section cardinality before copying, complete-response encoding, and preservation of editor state. Batch byte compilation rejects warnings, and the complete repository quality gate must remain green.

## Revisit when

Revisit before exposing durable compilation outcomes, output, commands, directories, global buffer selection, navigation, process control, shell execution, transport, jobs, cancellation, or mutation.
