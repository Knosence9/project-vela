# ADR-0013: Pause and resume durable workflow runs with revision-bound reasons

- **Status:** accepted
- **Date:** 2026-07-28
- **Decision owners:** Project Vela maintainers
- **Related:** ADR-0009, ADR-0010, ADR-0011, ADR-0012, issues #699 and #700

## Context

Durable workflow runs can start, advance, cancel, replay, and be discovered, but an unchanged non-terminal run cannot distinguish an intentional recoverable hold from abandoned work. It also remains eligible for advancement by any caller holding its current revision. The workflow plans reserve pause and resume as explicit lifecycle decisions, while the existing durable mutation boundary already uses exact revisions to prevent stale phase-relative intent from being reinterpreted.

## Decision

`WorkflowRunStore::pause` and `WorkflowRunStore::resume` are explicit revision-bound mutations. Both load the exact run, require the caller-observed revision, validate the current lifecycle state, and append with `ExpectedVersion::Exact`. A stale revision or append race returns the existing typed concurrent-modification error without retry.

A non-terminal, non-cancelled, active run may be paused with a non-empty exact UTF-8 reason. A paused, non-cancelled run may be resumed with a separate non-empty exact reason. Version-one `workflow_run.paused` and `workflow_run.resumed` events own the exact current phase index and caller reason. Pause and resume increment revision but never change the immutable topology or current phase. Projection exposes the current pause reason only; the event history retains both pause and resume rationale.

Paused runs reject advancement before transition or gate interpretation, but remain cancellable. Cancellation remains terminal for caller-controlled execution and may preserve the pause marker as the state at cancellation; a cancelled run cannot resume. Workflow-authored terminal runs cannot pause. Duplicate pause and resume-without-pause fail as typed lifecycle errors.

Replay validates non-empty reasons, exact phase provenance, and legal event ordering. Advancement while paused, duplicate pause, resume without pause, pause at a workflow-terminal phase, or any event after cancellation makes the complete history invalid. Exact loading and deterministic listing reuse the same fail-closed projector.

Pause and resume are lifecycle evidence, not orchestration authority. They do not schedule work, invoke actions, evaluate gates, bind skills, tools, tasks, providers, agents, or humans, grant permissions, retry, time out, add actors or timestamps, or resume automatically.

## Alternatives considered

### Treat inactivity as pause

Inactivity has no durable intent, reason, or advancement guard. It cannot support deterministic recovery or distinguish an intentional checkpoint from abandonment.

### Pause by adding a phase

A pause is run lifecycle state, not authored workflow topology. Rewriting or synthesizing phases would break immutable definition provenance and alter transition semantics.

### Allow advancement while paused

That would make pause informational only and permit another caller to bypass the explicit hold. Resumption must be the revision-changing operation that restores advancement eligibility.

### Clear pause when cancelling

Keeping the current pause marker records that cancellation occurred from a held run and avoids inventing an implicit resume event. Cancellation independently prevents all later lifecycle mutations.

## Consequences

- Callers can create recoverable, reason-bearing holds without changing workflow topology.
- Exact revisions serialize pause, resume, advance, and cancellation intent.
- Paused runs remain discoverable and cancellable while advancement is explicitly blocked.
- Replay detects malformed lifecycle ordering and phase provenance fail closed.
- The aggregate exposes current pause state but not a separate materialized resume-reason history; callers needing full audit detail use the durable event history boundary.

## Verification

The bounded execution slice follows RED→GREEN tests for reason validation, exact-revision pause/resume, reopen and listing projection, advancement blocking, cancellation while paused, terminal/cancelled/stale/duplicate failures, and malformed event replay. Existing workflow lifecycle tests and the complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding automatic resume, scheduling, actors, timestamps, timeouts, retries, action binding, task/provider/tool execution, permissions, remote execution, migration, or a public lifecycle-history query.
