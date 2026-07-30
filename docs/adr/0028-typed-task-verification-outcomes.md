# ADR-0028: Persist typed independent task Verification outcomes

- **Status:** accepted
- **Date:** 2026-07-30
- **Decision and execution issue:** [#769](https://github.com/Knosence9/project-vela/issues/769)

## Context

ADR-0027 separates independently observed Verification evidence from assistant responses and lifecycle authority, but its checker result is opaque text. Callers cannot distinguish a check that ran and failed from a checker that could not run without parsing prose or treating an execution error as evidence. The task event history must preserve that distinction without fabricating structure for older observations.

## Decision

`TaskVerificationOutcome` is a closed `Passed | Failed` taxonomy. A successful `TaskVerifier` invocation returns `TaskVerificationResult`, which carries one outcome plus opaque evidence text. `TaskVerifierError` continues to mean that the checker did not produce a result. `AssistantRuntime::verify_task_attempt` validates the evidence text and appends the outcome, evidence, fresh identity, and exact parent Attempt together through a narrow typed task-store operation.

`task.observation_appended` payload version 3 is reserved for structured Verification. It requires `kind: verification`, a non-blank parent Attempt ID, non-blank evidence text, and `verification_outcome: passed | failed`. Replay rejects missing or unknown outcomes and outcomes attached to another observation kind. Payload-version-1 ungrouped observations and payload-version-2 optionally parented observations remain replayable with no fabricated outcome. Existing generic append operations continue to emit payload version 2, including legacy opaque Verification evidence; only the typed operation emits version 3.

Both `Passed` and `Failed` are durable evidence about one Attempt. They leave the task active and do not complete, fail, or cancel it; mutate a workflow run; acknowledge a gate; or permit evidence after a terminal event.

## Alternatives considered

### Encode pass/fail into the evidence text

That would make correctness depend on prose parsing and would not distinguish checker execution failure from an observed failed check.

### Require every historical and generic Verification to have an outcome

Existing payload versions intentionally preserve opaque evidence and remain valid history. Rewriting streams or assigning inferred outcomes would violate append-only compatibility.

### Derive task completion from a passing result

Verification evidence and lifecycle intent are separate authorities. Completion policy needs explicit rules for required checks, stale attempts, multiple or conflicting outcomes, and races; it is not implied by this persistence slice.

## Consequences

- Callers can distinguish `Passed`, `Failed`, and checker execution error without parsing evidence text.
- Reopened task projections preserve exact structured outcomes while older observations expose `None`.
- The event payload version makes structured invariants fail closed without breaking legacy replay or generic append callers.
- Completion and workflow policy remain explicit follow-on contracts.

## Verification

RED→GREEN tests cover persisted and reopened passed and failed outcomes, exact payload version 3 encoding, failed-outcome versus execution-error behavior, blank evidence, payload-version-1/2 compatibility, malformed version-3 kind/outcome combinations, exact lineage, terminal rejection, provider/session isolation, and authoritative races without verifier retry. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding executable command/tool verifier adapters, durable verifier identity or artifacts, check freshness or supersession, task completion policy derived from checks, workflow gates, retries, scheduling, timestamps, actors, credentials, remote execution, or post-terminal evidence.
