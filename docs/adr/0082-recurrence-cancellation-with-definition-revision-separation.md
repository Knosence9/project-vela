# ADR-0082: Recurrence cancellation with definition-revision separation

- **Status:** accepted
- **Date:** 2026-08-06
- **Decision and execution issue:** [#959](https://github.com/Knosence9/project-vela/issues/959)
- **Related:** ADR-0037, ADR-0045, ADR-0060, ADR-0069, ADR-0072

## Context

Finite recurrence definitions are currently immutable and only carry one visible
`revision`, which implicitly doubles as both the authored definition revision
and the mutable aggregate revision. That ambiguity is acceptable while the only
recurrence-stream event is creation, but it becomes misleading once the
aggregate needs a durable cancellation decision that must not rewrite authored
occurrence provenance.

The recurrence lifecycle also already supports three categories of work:

- read-only authored projection
- persisted occurrence provenance and its claim/materialization lifecycle
- bounded due-selection mutations that can create new provenance

Issue #959 requires an explicit durable cancellation boundary that prospectively
blocks every new due-eligibility, provenance, claim, and direct-available
materialization mutation without hiding immutable authored definition state or
historical occurrence provenance. Claims established before cancellation must
remain recoverable so callers can still release them or consume them through the
separate claimed-materialization boundary.

## Decision

Add a version-one `recurrence.cancelled` event on the recurrence aggregate
stream with one exact non-blank caller-owned reason.

`FixedIntervalRecurrence` now exposes:

- explicit `RecurrenceStatus` with `Active` and `Cancelled`
- immutable `definition_revision`, fixed at the authored definition revision
- mutable aggregate `revision`, incremented by cancellation
- optional `cancellation`

Authored occurrence projection continues to carry `definition_revision`, not the
aggregate revision. Persisted occurrence provenance therefore remains anchored to
the immutable authored definition even after the recurrence aggregate advances.

`RecurrenceStore::cancel` accepts the exact recurrence ID, the caller-observed
aggregate revision, and the validated cancellation reason. It appends only when
the recurrence still exists at that exact revision and is not already
cancelled. Racing callers receive the existing recurrence concurrent-modification
error.

Cancellation has asymmetric authority effects:

- read-only authored recurrence projection and exact/bounded persisted,
  claimed, released, materialized, and task-provenance inspection remain
  available
- read-only due-selection projection returns no eligible work
- available-occurrence paging returns no directly available coordinates
- new provenance and direct-available mutations are rejected with
  `AlreadyCancelled`
- a claim established before cancellation may still be released or consumed by
  `materialize_claimed_occurrence`

No recurrence cancel CLI is added in this slice. Existing recurrence CLI read
models become truthful by exposing lowercase `status`, `definition_revision`,
`aggregate_revision`, and nullable `cancellation`.

## Alternatives considered

### Reuse one ambiguous recurrence revision

Rejected because persisted occurrence provenance would appear to change its
authored identity after aggregate cancellation, which is false.

### Model cancellation on each occurrence stream

Rejected because the decision is aggregate-wide prospective authority, not
per-coordinate authored provenance.

### Allow cancellation to block release or claimed materialization

Rejected because pre-cancellation claims would become unrecoverable durable
reservations with no caller-owned escape hatch.

### Add a recurrence cancel CLI immediately

Rejected because the issue only requires the kernel contract, truthful read-side
projection, and documentation. A writable CLI can be added later without
changing the durable model.

## Consequences

- Recurrence aggregates now distinguish immutable authored revision from mutable
  lifecycle revision.
- Cancellation fail-closes every future provenance and direct-available
  mutation, including race windows guarded by the recurrence stream revision.
- Historical provenance and authored recurrence structure remain inspectable
  after cancellation.
- Existing exact claimed-release and claimed-materialization recovery paths stay
  usable for work reserved before cancellation.

## Verification

RED→GREEN focused kernel and CLI tests prove:

- cancellation replays with separate definition and aggregate revisions
- kernel recurrence projection exposes explicit active/cancelled lifecycle status
- read-only due projection and available paging show no new eligibility after
  cancellation
- new provenance, claim-next, exact claim, and direct-available materialization
  mutations reject a cancelled recurrence
- pre-cancellation claims can still be released and historical provenance stays
  inspectable
- recurrence CLI lookup and inventory project `definition_revision`,
  lowercase `status`, `aggregate_revision`, and `cancellation` truthfully

## Revisit when

Reconsider before adding a recurrence cancel CLI, recurrence history APIs,
cross-recurrence cancellation workflows, undo semantics, skip evidence, or
other mutable recurrence-stream events beyond cancellation.
