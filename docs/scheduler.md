# Durable one-shot task schedules

The `vela-kernel` crate provides an inert first scheduler boundary: callers can persist one-shot task intent and deterministically query which intents are due. The boundary records intent only; it does not read a clock or execute work.

## Observable contract

- `ScheduleId` is an opaque UTF-8 identity containing at least one non-whitespace character. Exact content is preserved; IDs are not trimmed, case-folded, or normalized. The store isolates owning streams with the internal `schedule:` prefix.
- `ScheduleInstant` is an exact non-negative `u64` count of Unix milliseconds. The kernel does not convert civil times, interpret time zones, or read ambient time.
- `ScheduledTask` preserves the exact schedule ID, validated `TaskGoal`, and due instant supplied by the caller.
- `ScheduleStore::schedule` appends one `schedule.created` event at payload version `1` with `ExpectedVersion::NoStream`. A duplicate ID returns `ScheduleStoreError::AlreadyExists` and leaves the original intent unchanged.
- `ScheduleStore::load` replays one exact stream. A missing stream returns `None`; reopening the database returns an equal projection.
- `ScheduleStore::list_due(cutoff)` discovers schedules from existing creation events in one SQLite read snapshot. It includes due instants equal to the caller-owned cutoff and returns results ordered by due instant, then exact schedule ID. Future intents and unrelated streams are excluded; an empty store returns an empty list.
- Replay is fail-closed. Malformed creation payloads, unsupported event types or payload versions, invalid owning stream IDs, and histories other than exactly one valid creation event return typed errors instead of partial results.
- Every stored schedule is currently pending because no other schedule lifecycle event exists. The API does not infer a status from wall-clock time.

## Authority boundary

The caller owns the cutoff supplied to `list_due` and every action taken from its result. Scheduling and due queries do not create a task, start or advance a workflow, call a provider, invoke a tool, grant permission, sleep, claim work, retry, or persist execution results.

Cancellation, claiming, dispatch, recurrence and cron syntax, time zones, distributed leases, and terminal schedule outcomes are intentionally deferred. See [ADR-0034](adr/0034-durable-one-shot-task-schedule-intent.md) for the decision and rationale.
