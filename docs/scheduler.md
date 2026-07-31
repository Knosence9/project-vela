# Durable one-shot task schedules

The `vela-kernel` crate provides an inert first scheduler boundary: callers can persist one-shot task intent and deterministically query which intents are due. The boundary records intent only; it does not read a clock or execute work.

## Observable contract

- `ScheduleId` is an opaque UTF-8 identity containing at least one non-whitespace character. Exact content is preserved; IDs are not trimmed, case-folded, or normalized. The store isolates owning streams with the internal `schedule:` prefix.
- `ScheduleInstant` is an exact non-negative `u64` count of Unix milliseconds. The kernel does not convert civil times, interpret time zones, or read ambient time.
- `ScheduledTask` preserves the exact schedule ID, validated `TaskGoal`, due instant, and explicit `ScheduleStatus`. Cancelled schedules also preserve the exact cancellation reason supplied by the caller.
- `ScheduleStore::schedule` appends one `schedule.created` event at payload version `1` with `ExpectedVersion::NoStream`. A duplicate ID returns `ScheduleStoreError::AlreadyExists` and leaves the original intent unchanged.
- `ScheduleStore::cancel` appends one `schedule.cancelled` event at payload version `1` only for a pending schedule. Reasons require non-whitespace content without normalization. Missing, already-cancelled, and already-claimed schedules return typed errors without rewriting history.
- `ScheduleStore::claim(id, cutoff)` appends one empty `schedule.claimed` event at payload version `1` only when the schedule remains pending and its due instant is at or before the caller-owned cutoff. A future pending schedule returns typed `NotDue` evidence containing its due instant and the rejected cutoff. Missing, already-cancelled, and already-claimed schedules return typed errors without rewriting history; terminal errors take precedence over due-time evaluation. The operation does not read ambient time.
- `ScheduleStore::load` replays one exact stream. A missing stream returns `None`; reopening the database preserves pending, cancelled, or claimed status and any exact cancellation reason.
- `ScheduleStore::list_due(cutoff)` discovers schedules from existing creation events in one SQLite read snapshot. It includes pending due instants equal to the caller-owned cutoff and returns results ordered by due instant, then exact schedule ID. Cancelled, claimed, future, and unrelated streams are excluded; an empty store returns an empty list.
- Replay is fail-closed. Malformed creation, cancellation, or claim payloads, unsupported event types or payload versions, invalid owning stream IDs, and histories other than exactly `created`, `created -> cancelled`, or `created -> claimed` return typed errors instead of partial results.
- Cancellation and claiming are competing terminal transitions. Racing operations append exactly one event at stream version `2`; the loser reports the terminal state that won rather than overwriting it.
- Status changes only through persisted lifecycle events; due time alone never changes status.

## Authority boundary

The caller owns every cutoff supplied to `list_due` or `claim`, every cancellation and claim decision, and every action taken from a due or claimed result. Cancellation prevents future due selection but does not interrupt work already selected elsewhere. A claim is only a durable reservation: it does not create a task, start or advance a workflow, call a provider, invoke a tool, grant or revoke permission, sleep, retry, or persist execution results.

Dispatch, task creation, recurrence and cron syntax, time zones, worker identity, distributed leases, release/requeue, retries, and execution outcomes are intentionally deferred. A process failure after a successful claim therefore leaves the schedule claimed until a later recovery contract is introduced. See [ADR-0034](adr/0034-durable-one-shot-task-schedule-intent.md) for the decision and rationale.
