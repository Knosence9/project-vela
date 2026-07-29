# ADR-0017: Attribute workflow-run starts immutably to active tasks

- **Status:** accepted
- **Date:** 2026-07-28
- **Decision owners:** Project Vela maintainers
- **Related:** ADR-0009, ADR-0012, ADR-0014, issues #716, #717, #719, and #720

## Context

Durable workflow runs own immutable topology and caller-driven lifecycle evidence, while durable tasks own goals and execution evidence. Runs currently cannot record which task authorized their creation. Adding execution or automatic lifecycle synchronization would be premature because version-one workflow definitions have no phase actions, but later orchestration still needs trustworthy provenance rather than a caller-maintained side table.

The event log already supports appending one event only while another stream remains at an observed exact revision. The tool-invocation boundary uses that primitive to prevent attribution to a task that became terminal during intent persistence.

## Decision

Keep `WorkflowRunStore::start` for unassociated runs and add `start_for_task` for explicit task attribution. The associated start requires the exact task to exist and be `Active`. The store observes the task stream revision, then atomically creates the workflow-run stream only if the task stream remains unchanged. A missing task, terminal task, reused run ID, or racing task change returns a typed error and creates no run.

An associated start persists the exact task ID with the immutable workflow snapshot in payload version 2 of `workflow_run.started`. Payload-version-1 unassociated starts remain replayable without migration. `WorkflowRun::task_id` exposes the optional exact identity. Typed history preserves the existing `Started` shape for unassociated runs and adds `TaskStarted` for associated runs; exact loading and deterministic listing reuse the same fail-closed projector. A malformed persisted task ID fails replay.

Attribution is immutable because it is part of the authoritative start event and no reassociation event exists. After start, task and workflow-run lifecycles remain independent. Completing, cancelling, or failing either aggregate does not transition the other.

`WorkflowRunStore::list_for_task` provides a read-only exact-attribution query. It reuses the authoritative fail-closed projection and ascending run-ID order, excludes unassociated and differently attributed runs, and does not require the task to exist or remain active. This is historical provenance rather than execution authorization; malformed workflow-run history fails the whole query instead of returning partial results.

This boundary does not invoke phase actions, skills, tools, providers, agents, or humans; schedule work; choose transitions; evaluate gates; grant permission; create child tasks; synchronize lifecycle outcomes; retry; compensate; or infer success.

## Alternatives considered

### Store attribution in a later association event

A later event would permit an unattributed interval and require reassociation and concurrency semantics. Start-time provenance is simpler and matches the point at which the active task authorizes the run.

### Accept a task ID without validating the task

That would persist unverifiable provenance and allow new work to be attributed to missing or terminal tasks.

### Automatically synchronize terminal states

The aggregates have different semantics: authored workflow termination is not task success, and caller-owned workflow failure or cancellation need not be a task outcome. Synchronization requires a separate execution contract.

## Consequences

- Orchestration can discover exact task provenance after restart without a secondary index.
- Active-task validation and atomic revision guarding prevent attribution races.
- Legacy unassociated workflow runs remain valid.
- Callers can discover one task's attributed runs deterministically without a secondary index or caller-defined filtering semantics.
- Opening a workflow-run store also opens the task store over the same database.
- Callers remain responsible for both lifecycles after creation.

## Verification

The bounded execution slices follow RED→GREEN tests for active-task attribution, exact reopen/list/history projection, legacy unassociated replay, missing and terminal task rejection, a deterministic task-revision race that leaves no run stream, and exact task-filtered discovery after task termination and reopen. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding workflow execution ownership, phase action bindings, child-task creation, lifecycle synchronization, automatic transitions, scheduling, retries, compensation, actors, timestamps, migration, or remote execution.
