# ADR-0011: Cancel durable workflow runs with revision-bound reason events

- **Status:** accepted
- **Date:** 2026-07-28
- **Decision owners:** Project Vela maintainers
- **Related:** ADR-0009, ADR-0010, issues #693 and #694

## Context

ADR-0009 makes workflow-run topology and the starting phase durable, and ADR-0010 advances that topology with revision-bound transition events. Runs still have no explicit durable stop decision other than arriving at a workflow-authored terminal phase. The North Star requires explicit stop conditions, while the persisted task lifecycle already demonstrates that caller-owned reason-bearing cancellation is useful auditable state.

Cancellation must not be confused with a topology transition. A workflow definition may not contain a cancellation phase, and moving the cursor would falsely claim that a workflow-authored edge was selected. Cancellation also must not imply cooperative interruption, compensation, cleanup, or capability revocation when none of those execution contracts exists.

## Decision

A caller may cancel an existing workflow run only while it is at a non-terminal phase and has not already been cancelled. `WorkflowRunCancellation` owns one non-empty UTF-8 reason. Whitespace is meaningful and is preserved exactly.

`WorkflowRunStore::cancel` accepts the exact run ID, the caller-observed event-stream revision, and the validated reason. It loads and projects the owning stream, rejects a missing run, stale revision, workflow-authored terminal phase, or existing cancellation, then appends with `ExpectedVersion::Exact`. An append race returns the same typed concurrent-modification error used by advancement. The store does not retry or reinterpret stale cancellation intent.

A version-one `workflow_run.cancelled` event owns the authored current phase index and exact reason. The phase index is provenance into the immutable topology snapshot from the run's start event. Cancellation increments the run revision but does not change its topology or current phase. Projected state exposes the cancellation reason separately through `cancellation` and `is_cancelled`; `is_terminal` continues to mean only that the current workflow-authored phase is terminal.

Replay requires one start event followed by valid advancement events and at most one final cancellation event. It validates the persisted cancellation phase against the projected current phase and rejects cancellation at a terminal phase. Any duplicate start, cancellation after terminal arrival, duplicate cancellation, advancement after cancellation, malformed payload, unsupported event, phase mismatch, or other event after cancellation is invalid history. A persisted empty reason is malformed rather than silently accepted.

Cancellation records only a durable caller-owned lifecycle decision. It does not interrupt in-flight work, execute actions or compensation, revoke permissions or capabilities, schedule cleanup, derive a reason from model output, or identify an actor.

## Alternatives considered

### Model cancellation as a workflow transition

This would require every authored topology to include a cancellation phase and would falsely attribute an operational stop decision to a workflow-authored edge. It would also blur exact transition provenance.

### Allow cancellation after terminal arrival

A terminal phase is already the workflow definition's stop condition. Adding cancellation afterward would create two competing terminal interpretations without a completion lifecycle contract.

### Retry after optimistic-concurrency conflicts

Cancellation is caller intent against observed state. Retrying against a newer revision could stop a run after another caller advanced it, so stale intent must fail closed.

### Store only a cancelled boolean

A boolean loses the caller's audit reason and cannot validate which projected phase was stopped.

## Consequences

- Workflow runs gain an explicit durable, reason-bearing stop decision without changing authored topology.
- Racing advancement and cancellation persist at most one winner for an exact revision.
- Cancellation and terminal-phase arrival remain independently observable concepts.
- Cancelled runs reject all later advancement and cancellation.
- Callers must handle concurrent modification explicitly.
- Cooperative interruption, pause/resume, failure, compensation, cleanup, scheduling, actors, timestamps, and lifecycle discovery remain deferred.

## Revisit when

Reconsider this decision before adding cooperative execution interruption, compensation, capability revocation, cleanup orchestration, pause/resume, workflow failure, actors, timestamps, or a unified workflow terminal-status taxonomy.
