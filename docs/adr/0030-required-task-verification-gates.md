# ADR-0030: Evaluate required task Verification gates from durable evidence

- **Status:** accepted
- **Date:** 2026-07-30
- **Decision and execution issue:** [#773](https://github.com/Knosence9/project-vela/issues/773)

## Context

ADR-0029 gives each new independent task Verification a caller-owned check identity. Callers can now distinguish results without parsing evidence, but each caller would still need to reimplement the rules for deciding whether a required collection of checks is green. The architecture plan explicitly assigns verification-gate bookkeeping to deterministic code rather than model judgment.

The first useful policy can remain a pure view over one exact Attempt. Vela does not yet need executable checker definitions, persisted policy, automatic completion, workflow transitions, scheduling, or remote execution.

## Decision

`TaskVerificationGateSet` is a caller-owned, non-empty ordered collection of unique `TaskVerificationCheck` identities. Construction rejects an empty collection and exact duplicate identities while preserving authored order.

`Task::evaluate_verification_gates` validates that the selected observation exists and is an Attempt, then evaluates each required check from identified structured Verification observations linked to that exact Attempt. The latest matching observation in append order is authoritative for that check. No match is `Pending`; a match exposes its exact `Passed` or `Failed` outcome. Evidence for another Attempt and legacy or unidentified Verification observations cannot satisfy a gate.

The report preserves every required check and per-check status in caller order. Its overall status is `Failed` when any required check failed, otherwise `Pending` when any required check has no identified result, otherwise `Passed`. Failed precedence keeps a known failure visible even when another check has not run, while the per-check results preserve the complete mixed state.

Evaluation reads projected task state only. It appends no event, executes no command or tool, grants no permission, invokes no provider, and does not complete or fail a task or advance a workflow. Terminal task histories remain queryable.

## Alternatives considered

### Let every caller scan observations

That would duplicate latest-result, lineage, legacy compatibility, and aggregate-precedence rules in model or application code, contrary to the deterministic-bookkeeping principle.

### Treat any historical pass as sufficient

That would hide a later failed rerun. Append order is already durable and provides the only deterministic freshness relation currently available.

### Persist the required gate set on the task

Persisted policy introduces policy replacement, versioning, lifecycle timing, and authorization decisions. A caller-owned query is useful without committing to those contracts.

### Complete tasks automatically when all gates pass

Verification evidence is descriptive, not lifecycle authority. Automatic transitions would combine independent checking with caller-owned terminal decisions and require a separate concurrency and permission contract.

## Consequences

- Callers can deterministically inspect required verification state without parsing evidence text.
- Repeated checks have explicit latest-result semantics, scoped to one exact Attempt.
- Historical unidentified evidence remains truthful but cannot masquerade as satisfying an identified requirement.
- Required policy remains ephemeral and caller-owned; replayed task events and payload versions do not change.
- Executable checker definitions, artifacts, provenance, persisted gate policy, retries, and verification-derived transitions remain deferred.

## Verification

RED→GREEN tests cover gate-set validation and order, all-passed and mixed states, failure precedence, latest-result replacement, exact Attempt isolation, legacy evidence exclusion, invalid parent identities, and terminal-task readability. The complete repository quality gate must remain green.

## Revisit when

[ADR-0031](0031-verification-gated-task-completion.md) subsequently composes this pure report with an explicit caller-selected completion boundary that re-evaluates on concurrent task changes; gate evaluation itself remains read-only and caller-owned. Reconsider this decision before persisting or replacing gate policy, attaching commands or artifacts, deriving workflow lifecycle transitions, requiring freshness beyond append order, adding conditional/optional gates, or introducing actors, timestamps, scheduling, retries, permissions, credentials, or remote execution.
