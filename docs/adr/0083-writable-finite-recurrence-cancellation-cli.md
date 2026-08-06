# ADR-0083: Writable finite recurrence cancellation CLI

- **Status:** accepted
- **Date:** 2026-08-06
- **Decision and execution issue:** [#961](https://github.com/Knosence9/project-vela/issues/961)
- **Related:** ADR-0042, ADR-0071, ADR-0073, ADR-0082

## Context

ADR-0082 establishes revision-bound durable cancellation for one finite
recurrence aggregate while preserving immutable definition provenance and the
recovery paths for claims established before cancellation. The kernel and
read-side CLI projections expose that state, but an operator cannot yet cross
the mutation boundary through `vela-dev`.

## Decision

Add `vela-dev recurrence cancel DATABASE RECURRENCE_ID EXPECTED_REVISION
REASON` as a thin writable adapter over `RecurrenceStore::cancel`.

The adapter validates the exact non-blank recurrence identity and cancellation
reason before storage access. Clap parses the caller-observed aggregate revision
as a non-negative `u64`. The command opens only the caller-selected writable
store and delegates existence, exact optimistic concurrency, lifecycle replay,
and append semantics to the kernel.

Success emits the existing compact recurrence projection with exact ID and goal,
authored timing and count, lowercase `cancelled` status, immutable
`definition_revision`, incremented `aggregate_revision`, and exact escaped
cancellation evidence.

Invalid identity and reason inputs emit `invalid_recurrence_id` and
`invalid_recurrence_cancellation` before storage access. Missing, stale,
already-cancelled, malformed, open, replay, append, and serialization failures
emit `recurrence_cancellation_failed`, return non-zero, and emit no stdout.
Rejected transitions append no cancellation evidence.

This boundary prospectively withdraws future recurrence eligibility. It does not
erase the authored definition or historical occurrence lifecycle, interrupt
claims established before cancellation, read ambient time, generate identity,
dispatch, retry, grant permission, or execute work.

## Alternatives considered

### Compose raw event-log append in the CLI

Rejected because recurrence lifecycle replay, exact revision checks, and durable
event semantics belong to the kernel and already exist there.

### Hide cancelled recurrences or delete occurrence evidence

Rejected because cancellation is prospective authority withdrawal, not a
destructive migration or historical rewrite.

### Add cancellation history, undo, or cross-recurrence selection

Rejected as unrelated authority and policy. This slice adapts one existing exact
kernel mutation only.

## Consequences

- Operators can withdraw future recurrence eligibility through deterministic CLI
  JSON.
- The immutable definition revision remains distinct from the mutable aggregate
  revision.
- Existing claimed-release and claimed-materialization recovery boundaries remain
  available after cancellation.
- The CLI gains no clock, discovery, worker, dispatch, permission, or execution
  authority.

## Verification

RED→GREEN CLI integration tests prove successful exact-revision cancellation,
deterministic escaped projection, validation before storage, and missing, stale,
already-cancelled, and storage rejection without an extra append. The complete
repository quality gate must remain green.

## Revisit when

Reconsider before adding recurrence history, undo or resume semantics,
cross-recurrence cancellation, destructive deletion, claim interruption,
ambient clocks, workers, dispatch, permissions, or execution.
