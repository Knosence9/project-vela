# Durable one-shot task schedules

The `vela-kernel` crate provides an inert first scheduler boundary: callers can persist one-shot task intent and deterministically query which intents are due. The boundary records intent only; it does not read a clock or execute work.

## Observable contract

- `ScheduleId` is an opaque UTF-8 identity containing at least one non-whitespace character. Exact content is preserved; IDs are not trimmed, case-folded, or normalized. The store isolates owning streams with the internal `schedule:` prefix.
- `ScheduleInstant` is an exact non-negative `u64` count of Unix milliseconds. The kernel does not convert civil times, interpret time zones, or read ambient time.
- `ScheduledTask` preserves the exact schedule ID, validated `TaskGoal`, due instant, and explicit `ScheduleStatus`. Cancelled schedules also preserve the exact cancellation reason supplied by the caller; materialized schedules preserve the exact caller-owned task ID.
- `ScheduleStore::schedule` appends one `schedule.created` event at payload version `1` with `ExpectedVersion::NoStream`. A duplicate ID returns `ScheduleStoreError::AlreadyExists` and leaves the original intent unchanged.
- `ScheduleStore::cancel(id, expected_revision, reason)` appends one `schedule.cancelled` event at payload version `1` only when that exact revision remains pending. Reasons require non-whitespace content without normalization. Missing, already-cancelled, already-claimed, and stale schedules return typed errors without rewriting history.
- `ScheduleStore::claim(id, expected_revision, cutoff)` appends one empty `schedule.claimed` event at payload version `1` only when that exact revision remains pending and its due instant is at or before the caller-owned cutoff. A future pending schedule returns typed `NotDue` evidence containing its due instant and the rejected cutoff. Missing, already-cancelled, already-claimed, and stale schedules return typed errors without rewriting history; exact-revision validation precedes terminal-state and due-time evaluation. The operation does not read ambient time.
- `ScheduledTask::revision` exposes the exact persisted revision represented by a projection. `ScheduleStore::release(id, expected_revision, reason)` appends one `schedule.released` event at payload version `1` only when that exact revision remains claimed. Release reasons require non-whitespace content and preserve exact caller input as the latest recovery evidence. A release returns the immutable intent to pending eligibility, so it can appear in `list_due` and be claimed again. Missing, pending, cancelled, and stale schedules return typed errors without rewriting history; an earlier claimant cannot release a later claim after an intervening release/reclaim cycle.
- `ScheduleStore::materialize(id, expected_revision, task_id)` appends one `schedule.materialized` event and one existing `task.started` event atomically only when that exact revision remains claimed and the exact caller-owned task ID has no stream. The active task receives the schedule's exact immutable goal. Task-ID collisions leave the schedule claimed; stale schedule conflicts leave no orphan task, including when an earlier claimant acts after an intervening release/reclaim cycle. Materialized schedules are terminal and excluded from due discovery.
- `ScheduleStore::load` replays one exact stream. A missing stream returns `None`; reopening the database preserves pending, cancelled, claimed, or materialized status plus exact cancellation, latest release, and task-binding evidence.
- `ScheduleStore::open_read_only` exposes the existing load, inventory, status, due, history, and task-provenance projections through `EventLog::open_read_only`. It never creates a main database or initializes event schema. Mutation methods remain callable for API compatibility but fail closed at SQLite's read-only boundary and append no lifecycle evidence.
- `ScheduleStore::history` returns one exact schedule's complete validated lifecycle as revision-bearing typed creation, claim, release, cancellation, and materialization evidence in stream-revision order. It preserves exact goals, due instants, reasons, and task IDs; a missing schedule returns `None`. Complete decoding and lifecycle projection happen before any result is returned, so invalid history never yields a partial prefix.
- `ScheduleStore::find_by_task_id(task_id)` resolves the complete materialized schedule bound to one exact caller-owned task identity. An unrelated task returns `None`; duplicate corrupted bindings return typed `AmbiguousTaskBinding` evidence instead of an arbitrary result. The query validates discovered schedule histories in one read snapshot before returning provenance.
- `ScheduleStore::list` discovers every schedule from authoritative creation events in one SQLite read snapshot. It returns pending, cancelled, claimed, and materialized intents ordered by exact schedule ID, including exact lifecycle evidence, while excluding unrelated streams; an empty store returns an empty list.
- `ScheduleStore::list_by_status(status)` reuses the same read-only discovery boundary and returns only schedules whose explicit persisted status exactly matches the caller-owned filter, ordered by exact schedule ID. An unmatched status returns an empty list.
- `ScheduleStore::list_due(cutoff)` discovers schedules from existing creation events in one SQLite read snapshot. It includes pending due instants equal to the caller-owned cutoff and returns results ordered by due instant, then exact schedule ID. Cancelled, claimed, future, and unrelated streams are excluded; an empty store returns an empty list.
- Replay is fail-closed. Malformed creation, cancellation, claim, release, or materialization payloads, unsupported event types or payload versions, invalid owning stream IDs, and histories outside `created -> (claimed -> released)*` followed by an optional final claim, cancellation, or materialization from claimed state return typed errors instead of partial results.
- Cancellation and claiming are exact-revision competing transitions from pending. Racing operations append exactly one event; racing releases likewise append exactly one recovery event, and the loser receives typed `ConcurrentModification` evidence identifying the expected and persisted revisions. Racing materializations commit one complete schedule/task pair and no orphan stream. A complete intervening claim/release cycle that returns a schedule to the same status is reported through the same typed conflict rather than a raw event-log error.
- Status changes only through persisted lifecycle events; due time alone never changes status.

## Writable CLI creation

`vela-dev schedule create DATABASE SCHEDULE_ID GOAL DUE_AT_UNIX_MILLIS`
validates the exact caller-owned ID and non-empty task goal before opening the
exact caller-selected database through `ScheduleStore::open`. The due instant is
a parsed non-negative `u64`; invalid syntax is rejected by the CLI before the
command runs. Success appends exactly one `schedule.created` event and emits the
complete compact pending schedule object used by inspection, including revision
`1` and null lifecycle evidence. Exact caller-authored strings are JSON escaped.

Creation may initialize the selected database. Invalid IDs and goals emit
`invalid_schedule_id` or `invalid_task_goal` without creating storage. Duplicate
IDs, open, WAL, schema, append, and serialization failures emit one escaped
`schedule_creation_failed` diagnostic and no partial stdout; duplicate failure
does not replace the original intent. The command cannot read ambient time,
claim, cancel, release, materialize, dispatch, retry, or execute work.

## Writable CLI cancellation

`vela-dev schedule cancel DATABASE SCHEDULE_ID EXPECTED_REVISION REASON`
validates the exact caller-owned ID and non-blank cancellation reason before
opening the exact caller-selected database through `ScheduleStore::open`. The
revision is parsed as a non-negative `u64` by the CLI. Success delegates the
exact optimistic-concurrency check to `ScheduleStore::cancel`, appends one
`schedule.cancelled` event, and emits the complete compact cancelled schedule
object used by creation and inspection, including its resulting revision and
exact cancellation evidence.

Invalid IDs and reasons emit `invalid_schedule_id` or
`invalid_schedule_cancellation` without accessing storage. Missing schedules,
stale revisions, invalid lifecycle states, open, WAL, schema, append, replay,
and serialization failures emit one escaped `schedule_cancellation_failed`
diagnostic, non-zero status, and no partial stdout. Failed lifecycle operations
append no cancellation evidence. The command cannot read ambient time,
interrupt selected work, claim, release, materialize, dispatch, retry, or
execute a schedule or task. The expected revision identifies one persisted
observation; it is not worker identity, a lease, or a permission grant.

## Writable CLI claiming

`vela-dev schedule claim DATABASE SCHEDULE_ID EXPECTED_REVISION CUTOFF_UNIX_MILLIS`
validates the exact caller-owned ID before opening the exact caller-selected
database through `ScheduleStore::open`. The revision and cutoff are parsed as
non-negative `u64` values by the CLI before the command runs. Success delegates
the exact optimistic-concurrency and inclusive due-time checks to
`ScheduleStore::claim`, appends one `schedule.claimed` event, and emits the
complete compact claimed schedule object used by creation and inspection.

Invalid IDs emit `invalid_schedule_id` without accessing storage. Invalid
revision or cutoff syntax is rejected by the CLI before command execution.
Missing, future, stale, cancelled, materialized, or already-claimed schedules,
plus open, WAL, schema, append, replay, and serialization failures, emit one
escaped `schedule_claim_failed` diagnostic, non-zero status, and no partial
stdout. Failed lifecycle operations append no claim evidence. The cutoff is
caller-owned input: the command cannot read ambient time, identify a worker,
dispatch, release, materialize, retry, grant permission, or execute a schedule
or task. The expected revision is not worker identity, a lease, or proof of
liveness.

## Read-only CLI inspection

`vela-dev schedule inspect DATABASE` opens the exact caller-selected database
through `ScheduleStore::open_read_only` and emits one compact JSON document. Its
`schedules` array retains the kernel's deterministic exact-ID order. Every
object contains `id`, `goal`, `due_at_unix_millis`, lowercase `status`, and
`revision`; `cancellation`, `latest_release`, and `task_id` are exact strings
when present and JSON `null` otherwise. An empty inventory is
`{"schedules":[]}`. JSON serialization escapes untrusted identifiers, goals,
reasons, and task IDs.

Open, WAL, schema, replay, and projection failures emit one escaped
`schedule_inspection_failed` diagnostic and no stdout. In particular, inspecting
a missing path does not create a database. The command accepts no lifecycle
mutation, cutoff, dispatch, or execution options.

`vela-dev schedule get DATABASE SCHEDULE_ID` validates the exact schedule ID
before opening the caller-selected database read-only, then emits one compact
JSON document containing `id` and `schedule`. An existing schedule uses the same
complete deterministic object shape as inventory inspection; a valid missing ID
is represented by `"schedule":null`. Every exact caller-authored string is JSON
escaped during serialization.

Blank IDs fail before storage access with `invalid_schedule_id`. Open, WAL,
schema, replay, projection, and serialization failures emit one escaped
`schedule_lookup_failed` diagnostic and no partial stdout. Missing storage is
never created. Exact lookup cannot read time, mutate lifecycle state, dispatch,
retry, or execute a schedule or task.

`vela-dev schedule due DATABASE CUTOFF_UNIX_MILLIS` opens the same exact
caller-selected database read-only and supplies the parsed non-negative `u64`
cutoff to `ScheduleStore::list_due`. It emits the same compact JSON document and
complete schedule-object shape, but includes only pending schedules due at or
before the inclusive cutoff in due-instant then exact-ID order. An empty result
is `{"schedules":[]}`. Invalid cutoff syntax fails before storage is opened;
open, replay, projection, and serialization failures emit no partial stdout, and
a missing database is not created.

The due command does not read ambient time or infer that returned work should
run. It cannot mutate, claim, release, cancel, materialize, dispatch, retry, or
execute a schedule or task.

`vela-dev schedule status DATABASE STATUS` validates one exact lowercase
`pending`, `cancelled`, `claimed`, or `materialized` status before opening the
caller-selected database read-only, then supplies that typed status to
`ScheduleStore::list_by_status`. It emits the same compact complete schedule
objects as inventory inspection in exact-ID order. An unmatched status returns
`{"schedules":[]}`.

Invalid status input emits `invalid_schedule_status` without accessing storage.
Open, WAL, schema, replay, projection, and serialization failures emit one
escaped `schedule_status_inspection_failed` diagnostic and no partial stdout;
missing storage is never created. Status inspection cannot read time, mutate a
lifecycle, claim, dispatch, retry, materialize, or execute a schedule or task.

`vela-dev schedule history DATABASE SCHEDULE_ID` validates the exact schedule
ID before opening the caller-selected database read-only, then emits one compact
JSON document containing `id` and `history`. Existing histories are arrays in
exact revision order. Every entry contains `revision` and a lowercase `type`;
creation also contains exact `goal` and `due_at_unix_millis`, cancellation and
release contain exact `reason`, and materialization contains exact `task_id`.
JSON serialization escapes every caller-authored string. A valid missing ID is
represented by `{"id":"missing","history":null}` rather than an empty history.

Blank IDs fail before storage access with `invalid_schedule_id`. Open, WAL,
schema, replay, projection, and serialization failures emit one escaped
`schedule_history_failed` diagnostic and no partial stdout. Missing storage is
never created. History inspection cannot mutate lifecycle state, read ambient
time, dispatch, retry, or execute a schedule or task.

`vela-dev schedule task DATABASE TASK_ID` validates the exact task identity
before opening the caller-selected database read-only, then emits one compact
JSON document containing `task_id` and `schedule`. A materialized binding uses
the same complete deterministic schedule-object shape as inventory inspection;
an unbound valid identity is represented by `"schedule":null`. Accepted task
identities and every schedule field retain exact caller-authored content, with
JSON escaping applied during serialization.

Empty task IDs fail before storage access with `invalid_task_id`. Open, WAL,
schema, replay, ambiguity, projection, and serialization failures emit one
escaped `schedule_task_lookup_failed` diagnostic and no partial stdout. Missing
storage is never created. Task provenance lookup cannot mutate lifecycle state,
read ambient time, dispatch, retry, or execute a schedule or task.

## Authority boundary

The caller owns every status filter, every cutoff supplied to `list_due` or `claim`, every cancellation, claim, exact revision supplied to a mutation, task-ID binding decision, and action taken from a listed, exact, due, claimed, materialized, historical, or task-provenance result. Full, exact, status-filtered, historical, and task-provenance discovery are read-only and grant no authority; `open_read_only` additionally removes SQLite write and creation authority but is not a snapshot, secrecy boundary, or filesystem permission grant. Cancellation prevents future due selection but does not interrupt work already selected elsewhere. A revision identifies one exact persisted observation and prevents an earlier observer or claimant from consuming later lifecycle state; it is not worker identity, a permission grant, a lease, or proof of liveness. A claim is only a durable reservation, release is only caller-authored recovery evidence, and materialization only creates inert active task state: none infers worker health, starts or advances a workflow, calls a provider, invokes a tool, grants or revokes permission, sleeps, retries, or executes task work.

Dispatch, recurrence and cron syntax, time zones, worker identity, distributed leases, automatic claim expiry, retries, and execution outcomes are intentionally deferred. A process failure after a successful claim leaves the schedule claimed until an explicit caller releases or materializes it. See [ADR-0034](adr/0034-durable-one-shot-task-schedule-intent.md) for the decision and rationale.
