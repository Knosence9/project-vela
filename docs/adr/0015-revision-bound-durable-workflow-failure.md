# ADR-0015: Fail durable workflow runs with revision-bound diagnostics

- **Status:** accepted
- **Date:** 2026-07-28
- **Decision owners:** Project Vela maintainers
- **Related:** ADR-0010, ADR-0011, ADR-0013, ADR-0014, issues #707 and #708

## Context

Durable workflow runs can start, advance, pause, resume, cancel, reach workflow-authored terminal phases, and expose typed lifecycle history. They cannot represent an unsuccessful run outcome distinct from caller cancellation. Reusing cancellation would erase whether a caller intentionally stopped work or reported that work failed. Inferring failure from inactivity or external execution would add authority and evidence that the kernel does not own.

The existing lifecycle establishes immutable topology, exact-revision optimistic mutations, caller-owned reason evidence, and all-or-nothing replay. Failure should preserve those boundaries.

## Decision

`WorkflowRunFailure` owns one non-empty exact UTF-8 caller diagnostic. `WorkflowRunStore::fail` accepts an exact run ID, caller-observed revision, and failure diagnostic. It loads and validates the run, rejects stale revisions without retry, and appends with `ExpectedVersion::Exact`.

A version-one `workflow_run.failed` event owns the exact current phase index and diagnostic. Failure increments the stream revision but never changes the immutable topology or current phase. An active or paused non-terminal, non-cancelled run may fail. Failure while paused preserves the pause marker; it does not invent a resume event.

Failure is a terminal lifecycle condition distinct from workflow-authored terminal arrival and cancellation. A failed run rejects every later advance, pause, resume, cancel, or fail mutation with `AlreadyFailed`. Authored-terminal and cancelled runs cannot fail.

Exact load and deterministic list expose `is_failed` and the exact failure diagnostic. Typed lifecycle history exposes `Failed { phase_id, failure }`, translating internal phase provenance to the semantic authored phase ID.

Replay validates non-empty diagnostics, exact phase provenance, non-terminal source state, and legal ordering. Empty or malformed diagnostics fail decoding. A phase mismatch, failure at an authored terminal phase, duplicate failure, or any post-failure event invalidates the complete stream. Load, list, and history return no partial state or evidence.

Failure is caller-owned evidence only. It does not detect provider or tool errors, bind tasks or capabilities, interrupt work, retry, compensate, clean up, schedule, grant permission, add actors or timestamps, or execute workflow actions.

## Alternatives considered

### Encode failure as cancellation

Cancellation records intentional caller stop rationale. Failure records an unsuccessful outcome. Conflating them would weaken audit and recovery semantics.

### Add a synthetic failure phase

Failure is run lifecycle state, not authored topology. Synthesizing a phase would rewrite definition provenance and create transitions the workflow author did not declare.

### Automatically fail on provider or tool errors

No workflow action-binding or execution-ownership contract exists. Automatic failure would invent causal authority across currently separate runtime boundaries.

### Clear pause state when failing

Clearing the marker would imply an unrecorded resume. Preserving it accurately records that failure occurred while the run was held.

## Consequences

- Callers can durably distinguish unsuccessful outcomes from cancellation and authored completion.
- Exact revisions serialize failure against advance, pause, resume, and cancellation intent.
- Reopened and listed aggregates retain exact failure and pause-at-failure evidence.
- History provides semantic phase identity without exposing storage payloads.
- Callers remain responsible for deciding when an external condition warrants failure.
- Retry, recovery, compensation, cleanup, execution binding, scheduling, actors, timestamps, and a unified terminal-status enum remain deferred.

## Verification

The bounded execution slice follows RED→GREEN tests proving exact diagnostic validation and persistence; active and paused failure; exact revision conflicts; reopen, list, and semantic history projection; missing, cancelled, authored-terminal, and already-failed rejection; every post-failure mutation guard; malformed diagnostic and phase provenance rejection; and post-failure history rejection. Existing workflow-run tests and the complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding automatic failure detection, task/provider/tool binding, action execution, retries, recovery, compensation, cleanup, cooperative interruption, scheduling, timeouts, actors, timestamps, migration, remote execution, or a unified workflow terminal-status taxonomy.
