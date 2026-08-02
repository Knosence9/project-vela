# ADR-0037: Durable finite fixed-interval recurrence definitions

- **Status:** accepted
- **Date:** 2026-08-02
- **Decision and execution issue:** [#859](https://github.com/Knosence9/project-vela/issues/859)
- **Related:** ADR-0034, ADR-0035, ADR-0036

## Context

ADR-0035 and ADR-0036 define overflow-safe fixed-interval arithmetic without persistence. Persisting recurrence next requires explicit bounds, identity, and overflow behavior, but occurrence generation would additionally require lifecycle, provenance, catch-up, and materialization decisions.

An inert finite definition is the smallest durable boundary. A positive occurrence count gives every definition a representable final offset that can be validated before storage, without inventing infinite-range or missed-run policy.

## Decision

`RecurrenceId` is an exact non-blank UTF-8 identity. Recurrence streams use the dedicated internal `recurrence:` namespace and cannot collide with one-shot `schedule:` streams. `OccurrenceCount` is an exact positive `u64`; zero is invalid.

`FixedIntervalRecurrence` immutably preserves its ID, validated task goal, anchor `ScheduleInstant`, positive `ScheduleInterval`, occurrence count, final occurrence, and persisted revision. Offsets are zero-based: a count of one contains only offset zero at the anchor, and the final offset is `count - 1`.

`RecurrenceStore::create` validates the final occurrence with `ScheduleInstant::checked_advance_by` before persistence. Exact ranges ending at `u64::MAX` are accepted. Overflow returns typed `OccurrenceOverflow` evidence preserving the recurrence ID and arithmetic operands, and no stream is written. Successful creation appends one `recurrence.fixed_interval_created` event at payload version `1` with `ExpectedVersion::NoStream`. Duplicate identity preserves the original definition.

`RecurrenceStore::load` strictly decodes and validates one creation event. Unknown fields, blank goals, zero interval or count, overflowing ranges, unsupported event versions, and histories with any shape other than exactly one creation fail closed. A missing stream returns no definition.

This boundary is inert. It reads no ambient clock and does not enumerate or persist occurrences, assign occurrence or task identities, choose missed-run or catch-up behavior, cancel definitions, claim, materialize, dispatch, retry, or execute work.

## Alternatives considered

### Persist an unbounded recurrence

Rejected because a finite `u64` instant domain means every positive fixed interval eventually exceeds the representable range. An implicit overflow terminal would make authored intent and completion ambiguous.

### Reuse one-shot schedule identities and streams

Rejected because a recurrence definition and a one-shot lifecycle have different history contracts. Dedicated identities and stream prefixes prevent accidental cross-decoding while later occurrence provenance remains undecided.

### Generate occurrences in the creation slice

Rejected because generation requires durable occurrence identity, idempotency, catch-up bounds, lifecycle transitions, and task provenance. Creation and replay can establish the immutable authored definition without granting those authorities.

## Consequences

- Finite fixed-interval intent can be stored and reopened with exact arithmetic bounds.
- Every accepted definition has a representable anchor and final occurrence.
- The durable version-1 format commits to finite count semantics and a dedicated recurrence namespace.
- Occurrence identity, generation, discovery, lifecycle, catch-up, and execution remain explicit future decisions.

## Verification

RED→GREEN tests prove exact creation and reopen, positive count validation, zero-based final occurrence, exact `u64::MAX` acceptance, overflow before persistence, duplicate preservation, strict payload decoding, and invalid-history rejection. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding infinite recurrence, occurrence streams or identity, enumeration, cancellation, missed-run or catch-up policy, calendar units, cron parsing, time zones, automatic task identities, or ambient clock authority.
