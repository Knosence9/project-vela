# ADR-0021: Preserve explicit workflow-phase responses as task Attempt evidence

- **Status:** accepted
- **Date:** 2026-07-29
- **Decision owners:** Project Vela maintainers
- **Related:** ADR-0017, ADR-0019, ADR-0020, issues #745 and #746

## Context

ADR-0020 lets a caller explicitly apply one chosen workflow phase's registered skill bindings to a tool-free provider turn, but the successful response is durable only in its session transcript. Tasks separately own goals and execution evidence, and ADR-0017 permits a workflow run to carry immutable task attribution without making either aggregate's lifecycle control the other. Callers need a narrow way to preserve a phase-assisted response as task evidence without claiming that a workflow run executed or advanced.

## Decision

`AssistantRuntime::execute_workflow_phase_task_turn` is the explicit tool-free evidence boundary. The caller supplies the exact active task, fresh `TaskObservationId`, borrowed `RegisteredWorkflowPhase`, process-local skill registry, human content, system policy, and developer policy. The task must already be associated with a writable session.

Before transcript persistence or provider invocation, the runtime validates the task is active and associated, the Attempt identity is fresh and legal, the associated session is writable, and the phase bindings resolve through ADR-0019. The provider then receives ADR-0020's authority structure: system policy, developer policy, deterministic phase-bound registered skills, and the durable transcript after the human turn.

On success, durability is ordered: human transcript turn, provider call, response validation, then one SQLite transaction that appends both the assistant transcript turn and task Attempt. The Attempt text is the exact successful provider response. A provider failure or blank-only response therefore preserves the committed human turn and no assistant turn or Attempt. Racing task or session changes remain authoritative; if either rejects the final transaction, neither response record is appended.

The Attempt records provider response evidence only. It does not prove that the supplied phase belongs to a registered definition or durable run, persist the phase ID or selected skill IDs, authorize workflow work, establish workflow-run/task attribution, infer success, complete the task, or transition a workflow.

`AssistantRuntime::execute_workflow_phase_task_correction_turn` is the explicit corrective counterpart. The caller additionally selects an existing parent Attempt and supplies a fresh Correction ID. Active task association, exact Correction lineage, session writability, and phase bindings are validated before transcript or provider effects. A successful response is committed atomically as the assistant turn and exact Correction linked to that parent; it does not create another Attempt. Provider failure or blank-only output preserves only the committed human turn, while a racing final rejection appends neither response record. The borrowed phase remains caller-owned prompt authority and does not claim workflow-run provenance or mutate either lifecycle.

## Alternatives considered

### Accept a workflow-run ID and infer its task and current phase

That would combine event replay, task attribution, lifecycle eligibility, phase selection, provider authority, and evidence persistence. Keeping exact task and phase inputs caller-owned preserves the existing boundaries and avoids presenting historical attribution as execution authorization.

### Store a new workflow-phase execution event

There is not yet a durable execution identity or contract for phase attempts, retries, actors, outcomes, or lifecycle eligibility. Reusing task Attempt evidence records the useful response without inventing incomplete workflow execution semantics.

### Automatically advance or complete after a successful response

A provider response is not verification, transition selection, gate acknowledgement, task completion, or workflow completion. Those remain separate caller-owned decisions.

## Consequences

- Callers preserve one explicit phase-assisted response in both the associated session and the task's ordered evidence, or in neither aggregate when final persistence fails.
- Deterministic task, Attempt, session, and phase errors precede provider effects.
- Workflow-run lifecycle and task lifecycle remain independent.
- No durable phase provenance is claimed; callers needing that evidence require a later explicit schema and execution identity decision.

## Verification

The bounded slice follows RED→GREEN tests proving deterministic selected skills, durable session and Attempt evidence, duplicate Attempt and invalid phase rejection before side effects, provider-failure partial-commit semantics, and atomic rejection when an Attempt races the provider response. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding durable phase execution identities, workflow-aware tool turns, run-derived phase selection, persisted skill-selection evidence, task or workflow terminal synchronization, automatic transitions, retries, scheduling, actors, timestamps, or remote execution.
