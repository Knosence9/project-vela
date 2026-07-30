# ADR-0031: Complete tasks through caller-owned Verification gates

- **Status:** accepted
- **Date:** 2026-07-30
- **Decision and execution issue:** [#775](https://github.com/Knosence9/project-vela/issues/775)

## Context

ADR-0030 provides deterministic, read-only evaluation of a caller-owned required gate set against durable identified Verification evidence for one exact Attempt. Callers can inspect whether the gates are passed, pending, or failed, but composing that report with task completion themselves leaves a time-of-check/time-of-use gap: another Verification can be appended after the caller reads a green report but before unconditional completion.

The architecture plan assigns checkable gate bookkeeping to code rather than model judgment. Vela needs an explicit boundary for callers that choose verification-gated completion, without making gates global policy, removing unconditional caller authority, executing checkers, or coupling task and workflow lifecycles.

## Decision

`TaskStore::complete_if_verification_gates_pass` accepts a task ID, validated completion output, exact Attempt ID, and caller-owned `TaskVerificationGateSet`. It loads the current active task and evaluates the existing gate contract. Only an aggregate `Passed` report authorizes appending the existing `task.completed` event with the supplied exact output.

`Pending` and `Failed` return distinct `TaskVerifiedCompletionError` variants carrying the complete ordered report. Missing or non-Attempt identities preserve `TaskVerificationGateEvaluationError` as a typed source. Store, replay, and terminal-state failures preserve `TaskStoreError` as a typed source. Every rejection writes nothing.

The completion append uses the exact task stream version whose projected evidence was evaluated. If another task event wins first, the operation reloads the authoritative task and re-evaluates the gates. A newer failed Verification therefore blocks completion rather than permitting stale green evidence. A racing terminal transition returns its authoritative terminal-state error. The successful event and replay contract are identical to unconditional completion.

`TaskStore::complete` remains available. Gate policy is caller-owned and ephemeral, so this boundary is an explicit deterministic policy choice rather than a global task permission or persisted policy change.

## Alternatives considered

### Let callers evaluate then call unconditional completion

That exposes a stale-authorization race between the two public operations. Exact-version re-evaluation belongs in the store boundary that owns task concurrency.

### Make every completion verification-gated

Existing callers own terminal intent, and historical tasks have no persisted required gate policy. Changing unconditional completion would invent policy and break the established lifecycle contract.

### Persist required gates before completion

That requires policy lifecycle, replacement, versioning, and authorization decisions. The caller-owned gate set is enough to provide a safe explicit completion operation without adding events.

### Fail immediately on any concurrent task event

That is safe but makes callers repeat deterministic bookkeeping. Reloading and re-evaluating remains fail-closed while allowing an unrelated still-green append to converge without caller orchestration.

## Consequences

- Callers can choose an atomic authorization boundary between durable gate evidence and completion.
- Pending and failed reports remain inspectable without parsing messages.
- Exact-version retry cannot authorize completion from stale green evidence.
- Existing completion payloads, replay, and unconditional completion remain unchanged.
- The boundary executes no verifier, provider, command, or tool; grants no tool permission; and changes no workflow lifecycle.
- Persisted gate policy, artifacts, executable checker definitions, scheduling, retries outside exact concurrency re-evaluation, and automatic task or workflow transitions remain deferred.

## Verification

RED→GREEN tests cover all-passed completion and exact output replay, ordered pending and failed reports without writes, typed invalid-attempt causes, authoritative terminal state, and a deterministic race where a failed Verification lands after green evaluation but before append and blocks completion after re-evaluation. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before making gates mandatory for all completion, persisting or replacing gate policy, attaching executable checker definitions or provenance, advancing workflows from Verification, scheduling or retrying check execution, or introducing actors, credentials, timestamps, artifacts, or remote execution.
