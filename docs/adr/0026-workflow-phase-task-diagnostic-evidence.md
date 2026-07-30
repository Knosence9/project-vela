# ADR-0026: Preserve explicit workflow-phase diagnoses as linked task evidence

- **Status:** accepted
- **Date:** 2026-07-30
- **Decision and execution issue:** [#763](https://github.com/Knosence9/project-vela/issues/763)

## Context

Workflow-phase task turns preserve explicit provider responses as Attempt and Correction evidence and support caller-owned completion, failure, and cancellation. The task evidence model separately represents a `Diagnostic` linked to an earlier Attempt. The North Star requires evidence-producing improvement loops, so callers need a narrow way to ask one explicitly selected phase to diagnose one exact attempt without granting terminal, verification, tool, transition, or workflow-lifecycle authority.

## Decision

`AssistantRuntime::execute_workflow_phase_task_diagnostic_turn` is the explicit tool-free phase-assisted diagnostic boundary. The caller supplies the exact active associated task, an existing parent Attempt ID, a fresh Diagnostic observation ID, borrowed `RegisteredWorkflowPhase`, process-local skill registry, human content, system policy, and developer policy.

Before transcript persistence or provider invocation, the runtime validates that the task is active and associated, the Diagnostic identity and parent Attempt lineage are legal, the associated session is writable, and the phase bindings resolve through ADR-0019. The provider then receives ADR-0020's distinct authority fields and the durable transcript after the human turn.

On a valid response, the runtime validates the exact content as observation text and atomically appends the assistant transcript turn with a `Diagnostic` linked to the selected Attempt. Provider failure or blank-only content preserves the committed human turn and appends no assistant turn or Diagnostic. If task evidence changes after provider work, the atomic append writes neither the assistant response nor the requested Diagnostic and reports the authoritative task error.

The response is model-produced diagnostic evidence about one attempt. It is not independent `Verification`, a task-failure diagnostic, terminal intent, or proof that the diagnosis is correct. The operation does not infer or mutate a workflow run, persist phase provenance, acknowledge a gate, choose a transition, grant a tool, or change task or workflow lifecycle state.

## Alternatives considered

### Record the diagnosis as another Attempt

That would erase the distinction between work performed and analysis of why one exact attempt behaved as it did. The existing parented `Diagnostic` contract preserves that relationship directly.

### Treat the response as Verification evidence

A provider's own claim is not independent evidence that an attempt passed an external check. Verification remains caller-observed store-level evidence until a separate permission and provenance contract exists.

### Append the assistant turn and Diagnostic separately

A task race could leave an orphan assistant diagnosis that has no matching evidence record. Reusing the existing cross-stream atomic append boundary preserves a single observable outcome.

## Consequences

- Callers can explicitly diagnose one exact task Attempt through phase-selected skills.
- Exact Diagnostic lineage remains durable and replayable without creating a new Attempt.
- Deterministic task, lineage, session, and phase errors precede provider effects.
- A racing evidence change cannot orphan the assistant response.
- Task terminal state and workflow lifecycle remain caller-owned and unchanged.

## Verification

RED→GREEN tests prove deterministic selected skills, exact linked Diagnostic evidence, missing and non-Attempt parent rejection, duplicate identity and invalid phase rejection before effects, provider-failure partial commits, and atomic rejection during a racing Diagnostic append. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding independently observed runtime Verification, durable phase execution identities, run-derived phase selection, workflow-aware tools, task/workflow lifecycle synchronization, automatic transitions, retries, scheduling, actors, timestamps, or remote execution.
