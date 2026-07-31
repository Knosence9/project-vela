# ADR-0034: Durable inert one-shot task schedule intent

- **Status:** accepted
- **Date:** 2026-07-31
- **Decision and execution issues:** [#789](https://github.com/Knosence9/project-vela/issues/789), [#791](https://github.com/Knosence9/project-vela/issues/791)

## Context

Vela has durable task, workflow, and independent Verification lifecycles, but no code-owned scheduling primitive. The North Star assigns deterministic scheduling to code, and the architecture reserves scheduler/cron behavior for the inspectable kernel. Reading an ambient clock and dispatching work in the first scheduler operation would combine time authority, persistence, execution, retry, and permission decisions before their failure semantics exist.

The typed SQLite event log already provides exact stream identity, immutable append, replay, and snapshot-consistent discovery. A smaller first boundary can persist inert one-shot intent and deterministically report which intents are due relative to caller-owned input.

## Decision

`ScheduleStore` persists one immutable `schedule.created` event at payload version `1` for each exact `ScheduleId`. The event contains a validated `TaskGoal` and a `ScheduleInstant`, represented as an exact non-negative count of Unix milliseconds. Schedule IDs must contain at least one non-whitespace character; otherwise their exact UTF-8 is preserved and compared without normalization.

`ScheduleStore::schedule` uses `ExpectedVersion::NoStream`, so an exact ID can be created once and a duplicate cannot replace its original goal or due instant. `ScheduleStore::cancel` appends one version-one `schedule.cancelled` event with an exact non-blank caller reason only while the schedule is pending. Missing and already-cancelled schedules return typed errors without rewriting history. `load` replays one exact stream and projects explicit `Pending` or `Cancelled` status while preserving the cancellation reason. Histories other than exactly `created` or `created -> cancelled` fail closed.

`ScheduleStore::list_due(cutoff)` discovers streams from their authoritative creation events in one SQLite read snapshot. It returns every pending intent whose due instant is less than or equal to the caller-owned cutoff, ordered by due instant and then exact schedule ID. Cancelled and non-schedule streams are excluded. Invalid owning stream IDs, malformed payloads, unsupported events, and invalid histories are errors rather than skipped records.

The store never reads wall-clock time. It does not create a task, start or advance a workflow, invoke a provider or tool, grant permission, sleep, claim work, retry execution, or interpret recurrence, cron syntax, or time zones. A caller decides when to query and what to do with the returned inert intent.

## Alternatives considered

### Read `SystemTime::now` inside `list_due`

Rejected because ambient time makes selection less deterministic, hides clock authority inside persistence, and complicates tests. A caller-owned cutoff is explicit and can later come from a real or simulated clock.

### Create tasks automatically when schedules become due

Rejected because exactly-once identity, claiming, crash recovery, retry, cancellation, and task-creation atomicity are separate contracts. Persisting intent first avoids implying execution guarantees that the kernel does not yet provide.

### Start with cron expressions and time zones

Rejected because recurrence parsing and civil-time semantics add substantial policy before one-shot identity, replay, and ordering exist. A Unix-millisecond instant is sufficient for the first bounded slice.

### Store schedule rows outside the event log

Rejected because schedules require the same immutable identity and fail-closed replay properties as existing kernel aggregates. A separate mutable table would introduce a second persistence model without a demonstrated need.

## Consequences

- Vela can durably record exact one-shot task intent and query due work deterministically.
- Callers retain all clock and execution authority.
- Due results are stable across insertion order and database reopen.
- A pending schedule can be withdrawn exactly once with durable caller-owned reason evidence; cancelled schedules remain inspectable but are excluded from due work.
- Claiming, dispatch, recurrence, and execution outcomes require later explicit decisions.
