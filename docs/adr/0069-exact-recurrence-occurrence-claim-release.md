# ADR-0069: Exact recurrence occurrence claim release

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#933](https://github.com/Knosence9/project-vela/issues/933)
- **Related:** ADR-0034, ADR-0050, ADR-0068

## Context

ADR-0068 lets a caller durably reserve one exact persisted recurrence occurrence, but deliberately leaves that claim one-way. A failed or abandoned claimant therefore cannot record recovery or return the coordinate to eligibility. One-shot schedules already establish that recovery must be an explicit exact-revision transition with caller-authored evidence rather than inferred worker death or ambient lease expiry.

The smallest responsible recovery boundary releases only one exact claimed occurrence. Global claimed inventory, automatic expiry, claim-next selection, workers, and dispatch remain separate authorities.

## Decision

`RecurrenceOccurrenceRelease` validates one non-blank caller-authored reason while preserving exact input. `RecurrenceStore::release_occurrence(id, offset, expected_occurrence_revision, reason)` strictly replays one exact occurrence stream and validates the caller-observed revision before lifecycle state. The coordinate must currently be claimed and not materialized.

Success appends one version-1 `recurrence.occurrence_released` event containing the exact reason with `ExpectedVersion::Exact(expected_occurrence_revision)`. It returns `ReleasedRecurrenceOccurrence`, preserving the complete canonical occurrence, resulting revision, and latest release reason. `load_released_occurrence` exposes only the currently available released state after read-only reopen; a later claim or materialization makes that released-state lookup return `None`, while complete persisted provenance remains available through existing views.

Release restores available persisted provenance. A later exact claim may reserve the coordinate at the released revision, and existing direct exact materialization may bind it to a caller-owned inert task at that revision. Strict replay accepts `persisted -> (claimed -> released)*` followed by an optional final `claimed` or `materialized`; direct `persisted -> materialized` remains valid. Every other ordering fails closed.

Missing provenance, stale revisions, available state, materialized state, malformed evidence, read-only storage, and contention append nothing. Concurrent transitions against one observed revision commit at most one winner; a stale caller receives typed occurrence concurrent-modification evidence with the persisted revision.

A release reason is recovery evidence only. It does not identify a worker, prove failure, expire a lease, revoke permission, dispatch, retry, or execute work.

## Alternatives considered

### Infer release after elapsed time

Rejected because the kernel owns no ambient clock, worker-liveness oracle, or lease policy. A timeout cannot prove that selected work is abandoned.

### Keep released coordinates in a terminal recovery state

Rejected because release would not restore useful eligibility. Exact revisions and strict replay already provide the responsible boundary for reclaim or direct materialization.

### Add claim-next or claimed inventory

Rejected because global selection, ordering, corruption scope, and worker policy exceed one exact recovery transition.

### Materialize directly from claimed state in the same slice

Rejected because claimed-to-task consumption has a separate atomic task-stream contract. Release only returns the coordinate to the existing available boundary.

## Consequences

- Callers can recover an abandoned exact claim without inferring worker death.
- Exact release evidence remains durable and inspectable while the coordinate is available.
- Released coordinates can be reclaimed or directly materialized only at their exact current revision.
- Replay now supports repeated claim/release recovery cycles while rejecting impossible ordering.
- No automatic recovery, worker identity, or execution authority is introduced.

## Verification

Strict RED→GREEN integration tests cover exact UTF-8 release evidence, resulting revision, read-only reopen, reclaim, direct materialization after release, missing/stale/available/materialized failures, revision-before-lifecycle precedence, racing release, and malformed or impossible release histories. Existing recurrence tests and the complete repository quality gate remain green.

## Revisit when

Reconsider before adding claimed-to-task materialization, claimed inventory, claim-next selection, worker identity, leases or expiry, ambient clocks, CLI exposure, dispatch, retries, permissions, or execution.
