# Durable one-shot task schedules

The `vela-kernel` crate provides an inert first scheduler boundary: callers can persist one-shot task intent and deterministically query which intents are due. The boundary records intent only; it does not read a clock or execute work.

## Observable contract

- `ScheduleId` is an opaque UTF-8 identity containing at least one non-whitespace character. Exact content is preserved; IDs are not trimmed, case-folded, or normalized. The store isolates owning streams with the internal `schedule:` prefix.
- `ScheduleInstant` is an exact non-negative `u64` count of Unix milliseconds. The kernel does not convert civil times, interpret time zones, or read ambient time.
- `ScheduledTask` preserves the exact schedule ID, validated `TaskGoal`, due instant, explicit `ScheduleStatus`, and any exact cancellation reason supplied by the caller.
- `ScheduleStore::schedule` appends one `schedule.created` event at payload version `1` with `ExpectedVersion::NoStream`. A duplicate ID returns `ScheduleStoreError::AlreadyExists` and leaves the original intent unchanged.
- `ScheduleStore::cancel` appends one `schedule.cancelled` event at payload version `1` only for a pending schedule. Reasons require non-whitespace content without normalization. Missing and already-cancelled schedules return typed errors without rewriting history.
- `ScheduleStore::load` replays one exact stream. A missing stream returns `None`; reopening the database preserves pending or cancelled status and the exact cancellation reason.
- `ScheduleStore::list_due(cutoff)` discovers schedules from existing creation events in one SQLite read snapshot. It includes pending due instants equal to the caller-owned cutoff and returns results ordered by due instant, then exact schedule ID. Cancelled, future, and unrelated streams are excluded; an empty store returns an empty list.
- Replay is fail-closed. Malformed creation or cancellation payloads, unsupported event types or payload versions, invalid owning stream IDs, and histories other than exactly `created` or `created -> cancelled` return typed errors instead of partial results.
- Status changes only through persisted lifecycle events; due time alone never changes status.

## Authority boundary

The caller owns the cutoff supplied to `list_due`, every cancellation decision, and every action taken from a due result. Cancellation prevents future due selection but does not interrupt work already selected elsewhere. Scheduling and due queries do not create or cancel a task, start or advance a workflow, call a provider, invoke a tool, grant or revoke permission, sleep, claim work, retry, or persist execution results.

Claiming, dispatch, recurrence and cron syntax, time zones, distributed leases, and execution outcomes are intentionally deferred. See [ADR-0034](adr/0034-durable-one-shot-task-schedule-intent.md) for the decision and rationale.
