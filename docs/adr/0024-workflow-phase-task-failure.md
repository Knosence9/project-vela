# ADR-0024: Preserve explicit workflow-phase final responses as task failure attempts

- **Status:** accepted
- **Date:** 2026-07-30
- **Decision issue:** [#756](https://github.com/Knosence9/project-vela/issues/756)
- **Execution issue:** [#757](https://github.com/Knosence9/project-vela/issues/757)

## Context

ADR-0021 preserves one explicit workflow-phase response as task Attempt evidence, ADR-0022 provides the corrective counterpart, and ADR-0023 adds caller-owned completion. The task runtime separately models failure as caller-owned terminal intent: the final provider response is Attempt evidence, while an already validated caller diagnostic explains the failure. Callers need the same narrow phase-assisted failure boundary without granting a model authority to invent terminal diagnostics or coupling provider work to workflow-run lifecycle.

## Decision

`AssistantRuntime::fail_workflow_phase_task_turn` is the explicit tool-free phase-assisted task-failure boundary. The caller supplies the exact active task, fresh Attempt observation ID, validated `TaskFailure`, borrowed `RegisteredWorkflowPhase`, process-local skill registry, human content, system policy, and developer policy. The task must already be associated with a writable session.

Before transcript persistence or provider invocation, the runtime validates that the task is active and associated, the Attempt ID is fresh and legal, the associated session is writable, and the phase bindings resolve through ADR-0019. The provider then receives ADR-0020's distinct authority fields and the durable transcript after the human turn.

On a valid response, the runtime validates the exact content as Attempt text, atomically appends the assistant transcript turn and matching Attempt, then applies the caller-owned task failure with the exact retained diagnostic. A provider failure or blank-only response preserves the committed human turn and appends no assistant turn, Attempt, or failure. An authoritative failure while appending the response preserves neither assistant turn nor requested Attempt. A terminal race after that atomic append preserves the assistant turn and Attempt, returns the winning task error, and does not replace the winning terminal state.

Provider content is only Attempt evidence. It is not `Diagnostic` or `Verification` evidence and cannot replace the validated caller-owned `TaskFailure`. The operation applies only to the task. It does not infer or mutate a workflow run, prove phase or skill-selection provenance, acknowledge a gate, choose a transition, fail a workflow, grant tools, or synchronize task and workflow lifecycles.

## Alternatives considered

### Derive the failure diagnostic from the provider response

A model response can explain an attempt, but terminal diagnostics are caller-owned facts. Deriving the diagnostic would grant the model a new authority and blur execution evidence with the reason the caller chose failure.

### Fail the attributed workflow run with the task

Workflow-run attribution is historical context, not terminal synchronization authority. Coupling both failures would require explicit eligibility, revision, phase, diagnostic, and partial-failure contracts beyond this bounded operation.

### Persist the assistant turn, Attempt, and failure atomically

The existing terminal task runtime preserves ordered durable prefixes. Reusing that behavior keeps terminal races observable and avoids a special transaction contract for phase-assisted work.

## Consequences

- Callers can explicitly fail an active task after one phase-composed response.
- The exact response is durable Attempt evidence, while the exact caller diagnostic remains terminal authority.
- Deterministic task, session, and phase errors precede provider effects.
- The assistant turn and Attempt remain atomic, while task failure retains established ordered-prefix race semantics.
- No model-derived Diagnostic or Verification evidence is introduced, and workflow lifecycle remains caller-owned and unchanged.

## Verification

The bounded slice follows RED→GREEN tests proving deterministic selected skills, exact durable Attempt and caller diagnostic, duplicate-Attempt and invalid-phase rejection before provider effects, provider-failure partial-commit semantics, and preservation of the response prefix when failure loses a terminal race. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding durable phase execution identities, workflow-aware tool failure turns, run-derived phase selection, persisted skill-selection evidence, independent Verification capture, task/workflow terminal synchronization, automatic transitions, retries, scheduling, actors, timestamps, or remote execution.
