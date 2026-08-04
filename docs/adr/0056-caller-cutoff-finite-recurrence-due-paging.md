# ADR-0056: Caller-cutoff finite recurrence due paging

- **Status:** accepted
- **Date:** 2026-08-04
- **Decision and execution issue:** [#903](https://github.com/Knosence9/project-vela/issues/903)

## Context

ADR-0055 completes bounded operator inspection of materialized recurrence bindings. Finite definitions can project any caller-selected authored window, but scheduler callers that own a due cutoff must currently duplicate horizon filtering and cannot distinguish the temporary future horizon from the permanent finite end.

The smallest responsible next slice is one exact-recurrence, read-only kernel projection. Ambient clocks, global discovery, catch-up policy, generated identities, lifecycle mutation, and workers remain separate authorities.

## Decision

`RecurrenceStore::due_occurrences_page(id, start_offset, page_size, cutoff)` strictly loads one exact immutable recurrence definition. The caller supplies the exact recurrence identity, authored start coordinate, positive at-most-1024 allocation bound, and inclusive `ScheduleInstant` cutoff.

The result contains complete `RecurrenceOccurrence` values in ascending offset order while each deterministic occurrence instant is at or before the cutoff. Work and allocation are bounded by `OccurrencePageSize`. `next_offset` always identifies the first uninspected authored coordinate: it advances after a full page, equals the first future coordinate when the cutoff stops selection, and is `None` only after the finite definition end. A later caller-owned cutoff can therefore resume from a prior future-horizon cursor without rescanning earlier coordinates.

Missing definitions and malformed selected definition evidence fail closed through existing `RecurrenceStoreError` categories. A start at or beyond the finite count preserves `OccurrenceOutOfRange` evidence. Canonical exact occurrence projection preserves representability, provenance, and `u64::MAX` boundary behavior.

The operation reads no ambient time, persists no cursor or occurrence, and grants no global discovery, missed-run or catch-up choice, generated identity, materialization, cancellation, claim, lease, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Filter a normal occurrence page after projection

Rejected as a public contract because a filtered page's inherited cursor would skip future coordinates. The due boundary must stop at, and return, the first future authored coordinate.

### Return no cursor when nothing is currently due

Rejected because `None` would conflate a temporary caller-owned cutoff horizon with the permanent finite definition end and force callers to rediscover coordinates.

### Read the system clock in the store

Rejected because it would make projection nondeterministic and silently transfer cutoff and catch-up authority into the kernel.

## Consequences

- Callers can select bounded exact-recurrence due windows deterministically and resume when their cutoff advances.
- Inclusive due behavior and finite-end behavior are explicit at maximum representable instants.
- The projection does not discover recurrences globally or decide which missed occurrences should be persisted or materialized.
- A CLI adapter and any mutation or worker policy remain later bounded decisions.

## Verification

RED→GREEN integration tests cover an inclusive cutoff, a cutoff before the selected coordinate, bounded pages below the horizon, resume after a later cutoff, exact finite-end termination, read-only reopen, missing and out-of-range typed failures, and `u64::MAX` instants. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, global due recurrence discovery, durable catch-up cursors, missed-run policy, generated task identities, recurrence cancellation, claims or leases, ambient clocks, dispatch, retries, or execution.
