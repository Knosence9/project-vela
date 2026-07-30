# ADR-0022: Preserve explicit workflow-phase responses as task Correction evidence

- **Status:** accepted
- **Date:** 2026-07-30
- **Decision owners:** Project Vela maintainers
- **Related:** ADR-0019, ADR-0020, ADR-0021, issues #750 and #751

## Context

ADR-0021 preserves one explicit workflow-phase response as task Attempt evidence. Tasks separately model a Correction as response evidence linked to one earlier Attempt. A caller that requests phase-assisted corrective work needs the same explicit authority and durability boundary without misclassifying the response as another Attempt or coupling provider execution to workflow-run state.

## Decision

`AssistantRuntime::execute_workflow_phase_task_correction_turn` is the explicit tool-free correction boundary. The caller supplies the exact active task, existing parent Attempt ID, fresh Correction observation ID, borrowed `RegisteredWorkflowPhase`, process-local skill registry, human content, system policy, and developer policy. The task must already be associated with a writable session.

Before transcript persistence or provider invocation, the runtime validates that the task is active and associated, the Correction ID is fresh and legal, the parent exists and is an Attempt in the same task, the associated session is writable, and the phase bindings resolve through ADR-0019. The provider then receives ADR-0020's distinct authority fields and the durable transcript after the human turn.

On success, durability is ordered: human transcript turn, provider call, response validation, then one SQLite transaction that appends both the assistant transcript turn and exact Correction evidence linked to the supplied parent Attempt. A provider failure or blank-only response preserves the committed human turn and appends no assistant turn or Correction. Racing task or session changes remain authoritative; if either rejects the final transaction, neither response record is appended.

The Correction records provider response evidence only. It does not create an Attempt, prove that the supplied phase belongs to a registered definition or durable run, persist phase or skill-selection provenance, authorize workflow work, infer success, transition either lifecycle, grant tools, or constitute Verification evidence.

## Alternatives considered

### Record every phase response as an Attempt

That would erase the task aggregate's explicit correction lineage and make a caller-requested revision indistinguishable from a fresh execution attempt.

### Accept a workflow-run ID and infer the task, phase, and parent

That would combine historical attribution, lifecycle eligibility, prompt authority, and evidence lineage. Exact caller-owned inputs preserve the existing boundaries and deterministic validation order.

### Append the assistant turn and Correction separately

A task race could leave an orphan assistant answer that was rejected as Correction evidence. Reusing one atomic two-stream append preserves the response as both records or neither after provider completion.

## Consequences

- Callers can preserve one phase-assisted corrective response with exact parent-Attempt lineage.
- Deterministic task, lineage, session, and phase errors precede provider effects.
- Final assistant and Correction records are atomic, while the preceding human turn retains the established partial-commit semantics.
- Workflow-run and task lifecycle remain independent, and no phase provenance is claimed.

## Verification

The bounded slice follows RED→GREEN tests proving deterministic selected skills, durable session and exact Correction lineage, missing-parent, duplicate-Correction, and invalid-phase rejection before provider effects, and provider-failure partial-commit semantics. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding durable phase execution identities, workflow-aware tool turns, run-derived phase selection, persisted skill-selection evidence, task or workflow terminal synchronization, automatic transitions, retries, scheduling, actors, timestamps, or remote execution.
