# ADR-0035: Overflow-safe fixed-interval schedule arithmetic

- **Status:** accepted
- **Date:** 2026-08-02
- **Decision and execution issue:** [#853](https://github.com/Knosence9/project-vela/issues/853)
- **Related:** ADR-0034

## Context

ADR-0034 establishes durable one-shot schedule intent as exact non-negative Unix milliseconds and deliberately leaves recurrence for a later explicit decision. Persisted recurrence would need deterministic arithmetic before it could define occurrence timing, but ordinary unsigned addition can wrap and saturating addition would silently change authored timing. Starting with cron expressions or civil-time durations would also introduce parsing, calendars, time zones, and missed-run policy before the arithmetic boundary is explicit.

A smaller prerequisite can define one exact fixed interval and one checked advancement operation without changing persistence or granting scheduling authority.

## Decision

The scheduler exposes `ScheduleInterval` as an exact positive `u64` count of milliseconds. `ScheduleInterval::from_millis` rejects zero with `ScheduleIntervalError`; every positive value is preserved without normalization or unit conversion.

`ScheduleInstant::checked_advance(interval)` deterministically adds one interval to one caller-owned instant. An exactly representable sum returns the resulting `ScheduleInstant`, including `u64::MAX`. Overflow returns `ScheduleAdvanceError`, preserving the rejected instant and interval for typed inspection. Advancement never wraps or saturates.

This arithmetic boundary reads no ambient clock, interprets no civil time or time zone, and does not persist recurrence, create schedule or task identities, choose missed-run or catch-up policy, claim or materialize work, dispatch, retry, or grant worker authority.

## Alternatives considered

### Use wrapping or saturating addition

Rejected because wrapping can make a future occurrence appear in the distant past, while saturation can make distinct authored recurrences collapse onto one instant. Both hide an invalid recurrence calculation instead of exposing typed evidence.

### Permit zero intervals

Rejected because repeatedly advancing by zero cannot make temporal progress and would force a later recurrence loop to invent a stop policy.

### Start with cron expressions or calendar durations

Rejected because civil-time recurrence requires calendars, time zones, daylight-saving behavior, parser compatibility, and missed-run policy. Exact fixed milliseconds are deterministic and sufficient for the first recurrence prerequisite.

## Consequences

- Future fixed-interval recurrence can reuse one tested arithmetic rule.
- Callers receive exact typed overflow evidence instead of wrapped or saturated instants.
- No durable format or existing one-shot lifecycle changes.
- Persistent recurrence, occurrence identity, catch-up, dispatch, retry, and worker authority still require later explicit decisions.

## Verification

RED→GREEN tests prove zero rejection, exact positive preservation, ordinary advancement, the maximum exactly representable sum, and typed overflow preserving both operands. The complete repository quality gate must remain green.

## Revisit when

Reconsider this decision before adding calendar units, cron parsing, time zones, negative or fractional durations, persisted recurring schedules, occurrence generation, missed-run policy, catch-up bounds, automatic task identities, or ambient clock authority.
