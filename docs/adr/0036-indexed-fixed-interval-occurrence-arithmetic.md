# ADR-0036: Indexed fixed-interval occurrence arithmetic

- **Status:** accepted
- **Date:** 2026-08-02
- **Decision and execution issue:** [#855](https://github.com/Knosence9/project-vela/issues/855)
- **Related:** ADR-0034, ADR-0035

## Context

ADR-0035 defines one overflow-safe advancement by a positive fixed-millisecond interval. A future persisted fixed-interval schedule will need to derive an occurrence instant from an immutable anchor and caller-owned occurrence offset. Repeated one-step advancement would make the work proportional to the offset, while unchecked multiplication could wrap before otherwise checked addition sees the elapsed duration.

Persistence would also make offset-zero semantics difficult to change. A smaller prerequisite can define indexed arithmetic before introducing a durable recurrence format or lifecycle.

## Decision

`ScheduleInstant::checked_advance_by(interval, offset)` derives a zero-based fixed-interval occurrence. Offset `0` returns the exact anchor instant. Offset `1` is arithmetically equivalent to `checked_advance(interval)`. Larger offsets calculate `interval.millis() * offset` with checked multiplication and add that elapsed duration to the anchor with checked addition, without iteration.

Every exactly representable result succeeds, including `u64::MAX`. Overflow in either multiplication or addition returns `ScheduleOccurrenceError`, preserving the rejected anchor instant, interval, and offset for typed inspection. Indexed advancement never wraps or saturates. The existing one-step operation and its `ScheduleAdvanceError` remain source- and behavior-compatible.

The offset is caller-owned arithmetic input only. It is not a persisted occurrence identity, retry count, execution count, authorization, or claim. This boundary reads no ambient clock, persists no recurrence, chooses no catch-up or missed-run policy, generates no schedule or task identity, and does not claim, materialize, dispatch, retry, or execute work.

## Alternatives considered

### Repeatedly call one-step advancement

Rejected because deriving a distant indexed occurrence would require work proportional to the offset. Checked multiplication and addition define the same exact arithmetic in constant time and expose overflow directly.

### Make offsets one-based

Rejected because zero-based arithmetic preserves the authored anchor as occurrence offset zero and makes the formula unambiguous: `anchor + interval * offset`.

### Persist recurrence now

Rejected because persistence still requires explicit decisions about recurrence identity, lifecycle transitions, materialized occurrence provenance, catch-up bounds, and overflow handling at the lifecycle boundary. Arithmetic alone grants none of that authority.

## Consequences

- Future fixed-interval recurrence can derive indexed instants in constant time from an immutable anchor.
- Both multiplication and addition overflow fail closed with all caller operands preserved.
- Existing one-step callers and errors remain unchanged.
- Persistent recurrence, occurrence identity, catch-up, dispatch, retry, and worker authority remain deferred.

## Verification

RED→GREEN tests prove offset-zero identity, offset-one equivalence, ordinary indexed advancement, the maximum exactly representable instant, multiplication overflow, and addition overflow. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding persisted recurring schedules, occurrence identity or counters, missed-run and catch-up policy, calendar units, cron parsing, time zones, automatic task identities, or ambient clock authority.
