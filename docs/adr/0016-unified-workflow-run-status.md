# ADR-0016: Project one unified workflow-run lifecycle status

- **Status:** accepted
- **Date:** 2026-07-28
- **Decision owners:** Project Vela maintainers
- **Related:** ADR-0011, ADR-0012, ADR-0013, ADR-0014, ADR-0015, issues #711 and #712

## Context

Durable workflow runs expose authored terminal arrival, caller cancellation, pause state, and caller-owned failure through separate predicates and evidence accessors. Callers that inspect exact loads or deterministic lists would otherwise need to reconstruct a lifecycle classification. The existing `is_terminal` predicate intentionally describes arrival at an authored terminal phase, not cancellation or failure, so broadening it would silently change its established meaning.

Cancellation, pause, failure, discovery, and typed history contracts are now stable enough to define a complete read-only taxonomy without changing persistence or mutation authority.

## Decision

Add an evidence-free, non-exhaustive `WorkflowRunStatus` enum with exactly five current variants:

- `Active` for a non-paused run at a non-terminal authored phase;
- `Paused` for a paused run at a non-terminal authored phase;
- `AuthoredTerminal` for arrival at a phase authored as terminal;
- `Cancelled` for caller cancellation; and
- `Failed` for caller-owned failure.

`WorkflowRun::status` derives this value solely from already validated projected state. It appends no event and changes no schema. Failure and cancellation take precedence over a retained pause marker, so a run failed while paused reports `Failed` while `is_paused`, `pause_reason`, `is_failed`, and `failure` continue exposing the exact underlying evidence. Existing replay rules make failure, cancellation, and authored terminal arrival mutually exclusive.

The enum is `Copy` and carries no reason or diagnostic. Callers needing exact evidence use the existing typed accessors or lifecycle history. Existing `is_terminal`, `is_paused`, `is_cancelled`, and `is_failed` predicates retain their established semantics.

This projection does not schedule, execute, retry, compensate, infer outcomes, bind tasks or capabilities, grant permission, or add timestamps or actors.

## Alternatives considered

### Broaden `is_terminal`

Returning true for cancellation and failure would break the predicate's established topology-specific meaning and still would not distinguish lifecycle outcomes.

### Put reasons and diagnostics inside the status enum

A borrowing enum would complicate a simple classifier, while an owned enum would clone evidence unnecessarily. Exact evidence already has typed accessors and history entries.

### Persist a status event

Status is a deterministic projection of existing validated evidence. Another event would duplicate authority and create consistency rules without adding information.

### Collapse authored terminal arrival into success

A terminal phase is authored topology. Calling it success would infer outcome semantics the workflow definition does not currently declare.

## Consequences

- Exact loads and deterministic lists expose one machine-checkable lifecycle classifier.
- `AuthoredTerminal` keeps topology termination distinct from caller-owned cancellation and failure.
- Paused-at-failure evidence remains intact while classification is unambiguous.
- Existing storage, replay, mutation, and evidence contracts remain unchanged.
- Future enum variants can be added because the public enum is non-exhaustive.
- Callers still use typed accessors or history when they need exact reasons and diagnostics.

## Verification

The bounded execution slice follows RED→GREEN tests proving active, paused, resumed, authored-terminal, cancelled, active-failed, and paused-failed classification. Exact reopen and deterministic list paths must expose the same classifier, existing predicates and evidence must remain unchanged, and the complete repository quality gate must stay green.

## Revisit when

Reconsider this decision before adding new lifecycle outcomes, authored success/failure semantics, automatic execution ownership, retries, recovery, compensation, scheduling, actors, timestamps, migration, or remote execution.
