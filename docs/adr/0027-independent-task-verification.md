# ADR-0027: Record independently observed task Verification evidence

- **Status:** accepted
- **Date:** 2026-07-30
- **Decision and execution issue:** [#767](https://github.com/Knosence9/project-vela/issues/767)

## Context

Assistant task turns can preserve model responses as Attempt, Correction, or Diagnostic evidence. ADR-0026 deliberately rejects treating a model's own diagnosis as independent Verification. The task aggregate already models Verification linked to one earlier Attempt, while the North Star requires deterministic checks and evidence-producing improvement loops to remain code-owned.

A runtime caller therefore needs a narrow boundary for executing an independent checker against one exact attempt without routing the result through the assistant provider, transcript, workflow phase, or task lifecycle authority.

## Decision

`TaskVerifier` is a synchronous caller-owned boundary distinct from every assistant-provider trait. `AssistantRuntime::verify_task_attempt` accepts an active task ID, exact existing parent Attempt ID, fresh Verification observation ID, and borrowed verifier. The verifier receives a `TaskVerificationRequest` containing immutable references to the exact loaded task and parent Attempt.

Before verifier effects, the runtime validates the active task, fresh observation identity, and exact Attempt lineage. It then invokes the verifier once. A successful non-blank result is appended as `TaskObservationKind::Verification` linked to that Attempt. `TaskVerifierError` preserves a checker-specific standard error as its source; verifier failure and blank output append nothing. A racing task or evidence change remains authoritative at the task-store append and never causes a second verifier invocation.

Verification writes no session turn and never invokes the assistant provider. It does not require a session association, select or prove a workflow phase or run, infer a typed pass/fail status from opaque text, or transition the task or workflow lifecycle.

## Alternatives considered

### Record a workflow-phase assistant response as Verification

That would let the model attest to its own work and collapse the distinction between analysis and independently observed evidence. Workflow-phase responses remain Attempt, Correction, or Diagnostic evidence.

### Ask callers to append Verification directly through `TaskStore`

The store remains available for already-observed evidence, but it cannot define or test the execution boundary around a fallible checker. The runtime operation establishes preflight-before-effects, exact immutable checker input, error provenance, and no-retry semantics.

### Persist a typed pass/fail result now

The task observation schema at this decision point intentionally stored opaque evidence text. A status taxonomy required a separate compatibility contract and was not needed to establish the independent execution boundary. [ADR-0028](0028-typed-task-verification-outcomes.md) later adds the bounded `Passed | Failed` taxonomy while preserving older opaque observations and continuing to defer command identity, artifacts, provenance, and lifecycle policy.

## Consequences

- Deterministic or external checker adapters can observe one exact task Attempt without assistant authority.
- Known task and lineage errors precede checker effects.
- Checker failure and invalid output write no evidence, while successful text is durable and replayable with exact parent lineage.
- Concurrent task evidence remains authoritative and verifier work is never retried implicitly.
- Verification does not mutate transcripts or lifecycle state.

## Verification

RED→GREEN tests prove exact task and Attempt input, linked Verification persistence, unchanged active status and transcript, terminal and lineage rejection before checker effects, no-write checker failure and blank-output behavior, and authoritative racing evidence without retry. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding executable command/tool adapters, durable verifier identity or artifacts, task completion policy derived from checks, workflow gates driven by verification, retries, scheduling, timestamps, actors, credentials, remote execution, or post-terminal evidence. Typed outcomes are specified separately by [ADR-0028](0028-typed-task-verification-outcomes.md).
