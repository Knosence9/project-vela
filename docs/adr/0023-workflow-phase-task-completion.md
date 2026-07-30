# ADR-0023: Preserve explicit workflow-phase final responses as task completion output

- **Status:** accepted
- **Date:** 2026-07-30
- **Decision issue:** [#753](https://github.com/Knosence9/project-vela/issues/753)
- **Execution issue:** [#754](https://github.com/Knosence9/project-vela/issues/754)

## Context

ADR-0021 preserves one explicit workflow-phase response as task Attempt evidence, and ADR-0022 provides the corrective counterpart. The task runtime separately models completion as caller-owned terminal intent: the final provider response is Attempt evidence and exact task output, but it is not independent Verification evidence. Callers need the same narrow phase-assisted completion boundary without coupling provider work to workflow-run lifecycle.

## Decision

`AssistantRuntime::complete_workflow_phase_task_turn` is the explicit tool-free phase-assisted completion boundary. The caller supplies the exact active task, fresh Attempt observation ID, borrowed `RegisteredWorkflowPhase`, process-local skill registry, human content, system policy, and developer policy. The task must already be associated with a writable session.

Before transcript persistence or provider invocation, the runtime validates that the task is active and associated, the Attempt ID is fresh and legal, the associated session is writable, and the phase bindings resolve through ADR-0019. The provider then receives ADR-0020's distinct authority fields and the durable transcript after the human turn.

On a valid response, the runtime validates the exact content as both Attempt text and task output. It atomically appends the assistant transcript turn and matching Attempt, then applies the caller-owned task completion with that exact output. A provider failure or blank-only response preserves the committed human turn and appends no assistant turn, Attempt, or completion. An authoritative failure while appending the response preserves neither assistant turn nor requested Attempt. A terminal race after that atomic append preserves the assistant turn and Attempt, returns the winning task error, and does not replace the winning terminal output.

Completion applies only to the task. It does not append Verification evidence, prove that the phase belongs to a registered definition or durable run, persist phase or skill-selection provenance, infer or mutate a workflow run, acknowledge a gate, choose a transition, complete a workflow, grant tools, or synchronize task and workflow lifecycles.

## Alternatives considered

### Treat a successful provider response as Verification

A model response is execution evidence, not an independently observed quality result. Reclassifying it would erase the distinction between proposed output and evidence that the output satisfies a check.

### Complete the attributed workflow run with the task

Workflow-run attribution is historical context, not terminal synchronization authority. Coupling both transitions would require explicit eligibility, revision, phase, and partial-failure contracts beyond this bounded operation.

### Complete atomically with the assistant turn and Attempt

The existing terminal task runtime deliberately preserves ordered durable prefixes across caller-owned terminal transitions. Reusing that behavior keeps terminal races observable and avoids a new special transaction contract for phase-assisted work.

## Consequences

- Callers can explicitly finish an active task with one phase-composed response as exact Attempt evidence and exact output.
- Deterministic task, session, and phase errors precede provider effects.
- The assistant turn and Attempt remain atomic, while task completion retains established ordered-prefix race semantics.
- No Verification or phase provenance is claimed, and workflow lifecycle remains caller-owned and unchanged.

## Verification

The bounded slice follows RED→GREEN tests proving deterministic selected skills, exact durable Attempt and output, duplicate-Attempt and invalid-phase rejection before provider effects, provider-failure partial-commit semantics, and preservation of the response prefix when completion loses a terminal race. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding durable phase execution identities, workflow-aware tool completion turns, run-derived phase selection, persisted skill-selection evidence, Verification capture from independent checks, task/workflow terminal synchronization, automatic transitions, retries, scheduling, actors, timestamps, or remote execution.
