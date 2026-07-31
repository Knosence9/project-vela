# ADR-0033: Verified terminal workflow/task synchronization

- **Status:** accepted
- **Date:** 2026-07-30
- **Decision and execution issue:** [#781](https://github.com/Knosence9/project-vela/issues/781)

## Context

ADR-0031 permits an active task to complete after caller-selected durable Verification gates pass. ADR-0032 separately permits a task-attributed workflow run to advance through an authored gate after the corresponding identified Verification passes. Calling those boundaries sequentially can leave the workflow in its authored terminal phase while the attributed task remains active, or complete the task before the workflow reaches terminal, when the second append loses a race or fails.

The typed SQLite event log already supports appending one event to each of two streams in one transaction with exact expected versions. The kernel needs an additive policy boundary that composes the existing evidence and lifecycle contracts without weakening their independent low-level operations.

## Decision

`WorkflowRunStore::advance_and_complete_task_if_verification_passes` is the explicit terminal synchronization boundary. The caller supplies one exact workflow-run revision, authored transition index, exact task Attempt ID, and validated task output.

The run must be active, task-attributed, and positioned at a gated transition whose target is an authored terminal phase. The attributed task must be active. The transition's exact authored gate ID becomes the single required `TaskVerificationCheck`; evaluation uses ADR-0030 latest-result semantics for identified structured Verification linked to the exact supplied Attempt. Missing and non-Attempt identities retain the typed gate-evaluation errors. Pending and failed results retain complete inspectable reports. Unattributed runs, ungated transitions, non-terminal targets, and terminal tasks are rejected explicitly.

After a passing evaluation, the store appends the existing `workflow_run.advanced` event with the exact authored gate acknowledgement and the existing `task.completed` event with the caller's exact output in one SQLite transaction. Both appends are guarded by the workflow revision supplied by the caller and the task stream version used for evaluation. Success returns replay-equivalent workflow and task projections; no new event type or payload version is introduced.

If the task stream changes first, the operation reloads the task and re-evaluates the same Attempt and gate. A newer failed Verification therefore blocks both lifecycle writes, and a winning task terminal event is authoritative. The caller's exact workflow revision is never updated or reinterpreted: if that stream changes, the operation returns the existing concurrent-modification error. The two requested events are never partially persisted.

The existing raw `WorkflowRunStore::advance`, workflow-only `advance_if_task_verification_passes`, unconditional `TaskStore::complete`, and task-only `complete_if_verification_gates_pass` operations remain available and unchanged. This additive boundary executes no verifier, assistant provider, command, tool, or transition selection; grants no permission; and does not synchronize failure or cancellation.

## Alternatives considered

### Call verified advancement and verified completion sequentially

Rejected because either ordering exposes an observable partial lifecycle state if the second independent append fails or loses a race.

### Make every terminal workflow advancement complete its attributed task

Rejected because raw advancement is a caller-owned acknowledgement primitive, some callers intentionally manage workflow and task lifecycles independently, and not every terminal workflow has task Verification authority or a task output.

### Persist a new combined aggregate or event

Rejected because the existing workflow and task streams remain the authoritative lifecycle records and the event log already provides the required atomic two-stream transaction. A third projection would duplicate state and complicate replay.

## Consequences

- Callers can opt into fail-closed terminal synchronization without changing independent lifecycle APIs.
- Exact identified Verification is the only evidence authority for the authored gate.
- A successful operation cannot expose only one of the requested lifecycle events.
- Task evidence races are stale-safe; workflow intent remains bound to the caller's exact revision.
- The boundary is intentionally limited to a single authored gate and successful completion. Failure, cancellation, automatic transition choice, and multi-check authored policy remain future decisions.
