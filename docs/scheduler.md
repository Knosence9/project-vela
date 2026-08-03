# Durable task schedules and finite recurrence definitions

The `vela-kernel` crate provides inert scheduler boundaries: callers can persist one-shot task intent, query which one-shot intents are due, persist finite fixed-interval recurrence definitions through the kernel or CLI, inspect those definitions, project exact recurrence occurrences individually or through bounded pages, and persist exact occurrence provenance. These boundaries record or inspect intent only; they do not read a clock, choose catch-up policy, materialize recurrence work, or execute it.

## Observable contract

- `ScheduleId` is an opaque UTF-8 identity containing at least one non-whitespace character. Exact content is preserved; IDs are not trimmed, case-folded, or normalized. The store isolates owning streams with the internal `schedule:` prefix.
- `ScheduleInstant` is an exact non-negative `u64` count of Unix milliseconds. The kernel does not convert civil times, interpret time zones, or read ambient time.
- `ScheduleInterval` is an exact positive `u64` count of fixed milliseconds. Zero is rejected. `ScheduleInstant::checked_advance` adds one interval deterministically, accepts the maximum exactly representable sum, and returns typed `ScheduleAdvanceError` evidence preserving both operands when the sum would overflow; it never wraps or saturates. `ScheduleInstant::checked_advance_by` derives a caller-owned zero-based occurrence in constant time: offset zero preserves the anchor, offset one is arithmetically equivalent to one-step advancement, and checked multiplication plus checked addition either returns the exact instant (including `u64::MAX`) or typed `ScheduleOccurrenceError` evidence preserving the anchor, interval, and offset. See [ADR-0035](adr/0035-overflow-safe-fixed-interval-schedule-arithmetic.md) and [ADR-0036](adr/0036-indexed-fixed-interval-occurrence-arithmetic.md).
- `RecurrenceId` preserves one exact non-blank UTF-8 identity in the dedicated internal `recurrence:` stream namespace. `OccurrenceCount` is an exact positive `u64`; offsets are zero-based from the immutable anchor and the final offset is `count - 1`.
- `FixedIntervalRecurrence` preserves its exact ID, validated `TaskGoal`, anchor, interval, occurrence count, final occurrence, and persisted revision. `RecurrenceStore::create` validates that final occurrence before appending one version-1 `recurrence.fixed_interval_created` event with `ExpectedVersion::NoStream`. Exact ranges ending at `u64::MAX` succeed; overflow fails with typed operand evidence before persistence. Duplicate creation preserves the original definition. `load` returns no definition for a missing stream and otherwise requires exactly one strictly decoded, fully representable creation event. See [ADR-0037](adr/0037-durable-finite-fixed-interval-recurrence-definitions.md).
- `RecurrenceStore::open_read_only` opens existing recurrence evidence without creating a database or granting SQLite write authority. `list` discovers authoritative recurrence creation streams in one read snapshot, reuses exact history projection, excludes unrelated streams, and returns complete definitions ordered by exact recurrence ID. Empty storage returns an empty list; malformed owning stream IDs, payloads, events, or histories fail closed before any partial inventory is returned. The inventory reads no clock, persists no occurrences or cursors, and grants no lifecycle or execution authority. See [ADR-0040](adr/0040-read-only-finite-recurrence-inventory.md).
- `FixedIntervalRecurrence::occurrence_at(offset)` projects one exact caller-owned zero-based offset in constant time. A successful `RecurrenceOccurrence` preserves the exact recurrence ID, validated goal, offset, instant, and definition revision. Offset zero is the anchor and `count - 1` is the validated final occurrence, including `u64::MAX`; offsets at or above the count return typed evidence preserving the ID, rejected offset, and exact count. Projection alone is not durable occurrence evidence, a claim, or authority to create or execute work; callers must explicitly cross the `persist_occurrence` boundary to record the coordinate. Lookup reads neither storage nor ambient time. See [ADR-0038](adr/0038-exact-finite-recurrence-occurrence-projection.md).
- `FixedIntervalRecurrence::occurrences_page(start_offset, page_size)` projects a deterministic allocation-bounded page in increasing offset order. `OccurrencePageSize` accepts 1 through 1024; zero and larger values fail with typed validation evidence before allocation. The start must identify an authored occurrence, final pages truncate at the finite count, and `next_offset` names the first unreturned offset only when another page remains. Bound arithmetic cannot wrap, and every item reuses exact lookup to preserve full provenance. The cursor is read-only projection coordinates, not durable state, due or catch-up policy, or execution authority. See [ADR-0039](adr/0039-bounded-finite-recurrence-occurrence-paging.md).
- `RecurrenceStore::persist_occurrence(id, expected_revision, offset)` records one exact authored coordinate as version-1 `recurrence.occurrence_persisted` evidence after exact definition replay, revision validation, and authoritative projection. The canonical internal stream identity is a collision-free byte-length-prefixed recurrence ID plus offset; duplicates return typed evidence and never replace the first event. `load_occurrence` replays only that coordinate and requires its exact ID, definition revision, offset, goal, and instant to agree with the authoritative definition. Missing coordinates return `None`; malformed, unsupported, divergent, or multi-event histories fail closed. Persistence reads no clock and grants no catch-up, schedule/task materialization, claim, dispatch, or execution authority. See [ADR-0045](adr/0045-durable-exact-recurrence-occurrence-provenance.md).
- `ScheduledTask` preserves the exact schedule ID, validated `TaskGoal`, due instant, and explicit `ScheduleStatus`. Cancelled schedules also preserve the exact cancellation reason supplied by the caller; materialized schedules preserve the exact caller-owned task ID.
- `ScheduleStore::schedule` appends one `schedule.created` event at payload version `1` with `ExpectedVersion::NoStream`. A duplicate ID returns `ScheduleStoreError::AlreadyExists` and leaves the original intent unchanged.
- `ScheduleStore::cancel(id, expected_revision, reason)` appends one `schedule.cancelled` event at payload version `1` only when that exact revision remains pending. Reasons require non-whitespace content without normalization. Missing, already-cancelled, already-claimed, and stale schedules return typed errors without rewriting history.
- `ScheduleStore::claim(id, expected_revision, cutoff)` appends one empty `schedule.claimed` event at payload version `1` only when that exact revision remains pending and its due instant is at or before the caller-owned cutoff. A future pending schedule returns typed `NotDue` evidence containing its due instant and the rejected cutoff. Missing, already-cancelled, already-claimed, and stale schedules return typed errors without rewriting history; exact-revision validation precedes terminal-state and due-time evaluation. The operation does not read ambient time.
- `ScheduleStore::claim_next_due(cutoff)` selects pending work in the existing due-instant then exact-ID order and claims the earliest eligible revision. It returns `None` when no pending schedule is due. If a competing transition consumes the selected revision, selection restarts against validated durable state so concurrent callers reserve distinct available schedules instead of exposing the internal optimistic conflict. Every restart follows a persisted competing transition; malformed durable state fails closed before later work can be selected. The operation does not read ambient time, sleep, dispatch, or execute work.
- `ScheduleStore::materialize_next_due(cutoff, task_id)` selects the earliest pending due intent in the same order. It atomically appends `schedule.materialized` against the exact pending revision and a new `TaskEvent::Started` with `ExpectedVersion::NoStream`; success returns `Some(ScheduledTask)` containing the updated schedule revision and exact task binding. It returns `None` when no work is due. A competing schedule transition restarts selection; a task-ID collision consumes no schedule. Direct pending materialization and explicit claimed materialization are both valid histories. The operation generates no identity, reads no clock, grants no permission, and dispatches or executes no work.
- `ScheduledTask::revision` exposes the exact persisted revision represented by a projection. `ScheduleStore::release(id, expected_revision, reason)` appends one `schedule.released` event at payload version `1` only when that exact revision remains claimed. Release reasons require non-whitespace content and preserve exact caller input as the latest recovery evidence. A release returns the immutable intent to pending eligibility, so it can appear in `list_due` and be claimed again. Missing, pending, cancelled, and stale schedules return typed errors without rewriting history; an earlier claimant cannot release a later claim after an intervening release/reclaim cycle.
- `ScheduleStore::materialize(id, expected_revision, task_id)` appends one `schedule.materialized` event and one existing `task.started` event atomically only when that exact revision remains claimed and the exact caller-owned task ID has no stream. The active task receives the schedule's exact immutable goal. Task-ID collisions leave the schedule claimed; stale schedule conflicts leave no orphan task, including when an earlier claimant acts after an intervening release/reclaim cycle. Materialized schedules are terminal and excluded from due discovery.
- `ScheduleStore::load` replays one exact stream. A missing stream returns `None`; reopening the database preserves pending, cancelled, claimed, or materialized status plus exact cancellation, latest release, and task-binding evidence.
- `ScheduleStore::open_read_only` exposes the existing load, inventory, status, due, history, and task-provenance projections through `EventLog::open_read_only`. It never creates a main database or initializes event schema. Mutation methods remain callable for API compatibility but fail closed at SQLite's read-only boundary and append no lifecycle evidence.
- `ScheduleStore::history` returns one exact schedule's complete validated lifecycle as revision-bearing typed creation, claim, release, cancellation, and materialization evidence in stream-revision order. It preserves exact goals, due instants, reasons, and task IDs; a missing schedule returns `None`. Complete decoding and lifecycle projection happen before any result is returned, so invalid history never yields a partial prefix.
- `ScheduleStore::find_by_task_id(task_id)` resolves the complete materialized schedule bound to one exact caller-owned task identity. An unrelated task returns `None`; duplicate corrupted bindings return typed `AmbiguousTaskBinding` evidence instead of an arbitrary result. The query validates discovered schedule histories in one read snapshot before returning provenance.
- `ScheduleStore::list` discovers every schedule from authoritative creation events in one SQLite read snapshot. It returns pending, cancelled, claimed, and materialized intents ordered by exact schedule ID, including exact lifecycle evidence, while excluding unrelated streams; an empty store returns an empty list.
- `ScheduleStore::list_by_status(status)` reuses the same read-only discovery boundary and returns only schedules whose explicit persisted status exactly matches the caller-owned filter, ordered by exact schedule ID. An unmatched status returns an empty list.
- `ScheduleStore::list_due(cutoff)` discovers schedules from existing creation events in one SQLite read snapshot. It includes pending due instants equal to the caller-owned cutoff and returns results ordered by due instant, then exact schedule ID. Cancelled, claimed, future, and unrelated streams are excluded; an empty store returns an empty list.
- Replay is fail-closed. Malformed creation, cancellation, claim, release, or materialization payloads, unsupported event types or payload versions, invalid owning stream IDs, and histories outside direct `created -> materialized` or `created -> (claimed -> released)*` followed by an optional final claim, pending cancellation, or claimed materialization return typed errors instead of partial results.
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

## Writable CLI release

`vela-dev schedule release DATABASE SCHEDULE_ID EXPECTED_REVISION REASON`
validates the exact caller-owned ID and non-blank recovery reason before opening
the exact caller-selected database through `ScheduleStore::open`. The revision
is parsed as a non-negative `u64` by the CLI. Success delegates the exact
optimistic-concurrency and claimed-state checks to `ScheduleStore::release`,
appends one `schedule.released` event, and emits the complete compact pending
schedule object with its resulting revision and exact latest release evidence.

Invalid IDs and reasons emit `invalid_schedule_id` or
`invalid_schedule_release_reason` without accessing storage. Invalid revision
syntax is rejected before command execution. Missing, stale, pending,
cancelled, or materialized schedules, plus open, WAL, schema, append, replay,
and serialization failures, emit one escaped `schedule_release_failed`
diagnostic, non-zero status, and no partial stdout. Failed lifecycle operations
append no release evidence. The command cannot infer worker death, read ambient
time, expire a lease, claim, dispatch, materialize, retry, grant permission, or
execute a schedule or task. The reason is caller-owned recovery evidence only,
and the expected revision is not worker identity, a lease, or proof of liveness.

## Writable CLI materialization

`vela-dev schedule materialize DATABASE SCHEDULE_ID EXPECTED_REVISION TASK_ID`
validates both exact caller-owned identities before opening the exact selected
database through `ScheduleStore::open`. The revision is parsed as a non-negative
`u64` by the CLI. Success delegates exact optimistic concurrency, claimed-state,
and task-stream uniqueness checks to `ScheduleStore::materialize`, atomically
appends `schedule.materialized` and `task.started`, and emits the complete compact
materialized schedule object with its resulting revision and exact task binding.

Invalid identities emit `invalid_schedule_id` or `invalid_task_id` without
accessing storage. Invalid revision syntax is rejected before command execution.
Missing, stale, pending, cancelled, already-materialized, or task-colliding
inputs, plus open, WAL, schema, replay, append, and serialization failures, emit
one escaped `schedule_materialization_failed` diagnostic, non-zero status, and
no partial stdout. Every failed operation leaves both streams unchanged, so it
cannot create an orphan task. The command cannot read ambient time, infer worker
identity, dispatch, advance a workflow, call a provider or tool, retry, grant
permission, or execute work. The expected revision is not worker identity, a
lease, a permission grant, or proof of liveness.

## Writable CLI next-due claiming

`vela-dev schedule claim-next DATABASE CUTOFF_UNIX_MILLIS` opens the exact
caller-selected database through `ScheduleStore::open` and supplies the parsed
non-negative `u64` cutoff to `ScheduleStore::claim_next_due`. The kernel selects
pending work by due instant then exact schedule ID and retries only after a
persisted competing transition consumes the selected revision. Success emits
one compact JSON document whose `schedule` field is the complete claimed
schedule object, or `null` when no eligible work remains.

Invalid cutoff syntax fails before the command runs. Open, WAL, schema, replay,
claim, and serialization failures emit one escaped `schedule_claim_failed`
diagnostic and no partial stdout. Malformed durable state fails closed rather
than skipping work. The command does not read ambient time, generate task
identity, infer worker state, dispatch, materialize, grant permission, sleep, or
execute work. Existing exact-ID claiming remains available when the caller owns
selection and an observed revision.

## Writable CLI next-due materialization

`vela-dev schedule materialize-next DATABASE CUTOFF_UNIX_MILLIS TASK_ID`
validates the exact caller-owned task identity before opening the selected
database through `ScheduleStore::open`; clap parses the cutoff as a non-negative
`u64` before command execution. The kernel selects pending work by due instant
then exact schedule ID and atomically appends `schedule.materialized` plus
`task.started`. Success emits one compact JSON document whose `schedule` field
is the complete materialized projection, or `null` when no eligible work
remains.

Invalid task IDs emit `invalid_task_id` without accessing storage. Task-ID
collisions, open, WAL, schema, replay, append, and serialization failures emit
one escaped `schedule_materialization_failed` diagnostic and no partial stdout.
Task-ID collisions and lifecycle or storage failures before commit leave both the
selected schedule and task stream unchanged; the atomic kernel append never
persists only one side. The command does not read ambient time, generate
identity, dispatch, advance a workflow, call a provider or tool, grant
permission, retry work, or execute anything. Existing exact claimed-revision
materialization remains available when the caller owns selection and recovery
semantics.

## Writable recurrence CLI creation

`vela-dev recurrence create DATABASE RECURRENCE_ID GOAL ANCHOR_UNIX_MILLIS
INTERVAL_MILLIS OCCURRENCE_COUNT` validates the exact caller-owned recurrence ID,
non-empty task goal, positive fixed interval, and positive finite occurrence
count before opening the exact selected database through `RecurrenceStore::open`.
Clap parses the anchor, interval, and count as non-negative `u64` values before
command execution. Valid creation may initialize the database and delegates
final-occurrence overflow validation and duplicate protection to the kernel.

Success emits the same complete compact recurrence object used by inspection,
including exact `id` and `goal`, `anchor_unix_millis`, `interval_millis`,
`occurrence_count`, `final_occurrence_unix_millis`, and `revision`. Invalid IDs,
goals, intervals, and counts emit `invalid_recurrence_id`, `invalid_task_goal`,
`invalid_recurrence_interval`, or `invalid_occurrence_count` without creating
storage. Duplicate, overflow, open, schema, append, and serialization failures
emit one escaped `recurrence_creation_failed` diagnostic and no stdout; failed
creation cannot replace an existing definition or persist an overflowing one.

The command reads no ambient time, generates no identities, persists no
occurrence lifecycle, and cannot choose catch-up policy, materialize, claim,
cancel, dispatch, retry, grant permission, or execute work. See
[ADR-0042](adr/0042-writable-finite-recurrence-cli-creation.md).

## Read-only exact recurrence CLI lookup

`vela-dev recurrence get DATABASE RECURRENCE_ID` validates the exact ID before
storage access, opens only the caller-selected existing database through
`RecurrenceStore::open_read_only`, and replays only the selected recurrence
stream through `RecurrenceStore::load`. Success emits the same complete compact
recurrence object used by creation and inventory, preserving and JSON escaping
exact `id` and `goal` strings.

Invalid IDs emit `invalid_recurrence_id` before storage access. An absent
recurrence in a compatible store emits `recurrence_not_found`. Open, schema,
replay, projection, and serialization failures emit `recurrence_lookup_failed`.
Every failure is non-zero with one escaped diagnostic and no stdout; a missing
database is never created. Corruption in unrelated streams cannot block exact
lookup.

The command cannot enumerate unrelated streams, read ambient time, mutate
recurrence state, project or persist occurrences, choose catch-up policy,
generate identities, materialize, claim, cancel, dispatch, retry, grant
permission, or execute work. See
[ADR-0043](adr/0043-read-only-exact-finite-recurrence-cli-lookup.md).

## Read-only exact recurrence occurrence CLI paging

`vela-dev recurrence occurrences DATABASE RECURRENCE_ID START_OFFSET PAGE_SIZE`
validates the exact recurrence ID and the positive, at-most-1024 page size before
storage access. It opens only the selected existing database through
`RecurrenceStore::open_read_only`, loads only the selected recurrence stream,
and delegates bounded projection to `FixedIntervalRecurrence::occurrences_page`.

Success emits `occurrences` in ascending offset order. Every object preserves
exact `recurrence_id`, `goal`, `offset`, `unix_millis`, and
`definition_revision`; `next_offset` identifies the first unreturned authored
offset or is `null` on the final page. Invalid IDs, page sizes, and starts are
categorized as `invalid_recurrence_id`, `invalid_occurrence_page_size`, and
`recurrence_occurrence_out_of_range`. Absence emits `recurrence_not_found`;
storage, exact replay, and serialization failures emit
`recurrence_occurrence_lookup_failed`. All failures are non-zero with empty
stdout, missing storage is not created, and unrelated corruption cannot block
exact paging.

The offset is only a projection coordinate. The command reads no ambient time,
persists no occurrence identity or cursor, decides no due or catch-up state,
and cannot mutate, materialize, claim, cancel, dispatch, retry, grant permission,
or execute work. See
[ADR-0044](adr/0044-read-only-exact-finite-recurrence-occurrence-cli-paging.md).

## Writable exact recurrence occurrence provenance CLI

`vela-dev recurrence persist DATABASE RECURRENCE_ID EXPECTED_REVISION OFFSET`
validates the exact recurrence ID before storage access; clap parses the expected
definition revision and zero-based offset as non-negative `u64` values before
command execution. It opens only the caller-selected database through
`RecurrenceStore::open` and delegates exact definition replay, revision and
bounds validation, duplicate protection, projection, and append to
`RecurrenceStore::persist_occurrence`.

Success emits one compact occurrence object preserving exact `recurrence_id`,
`goal`, `offset`, `unix_millis`, and `definition_revision`. Invalid IDs emit
`invalid_recurrence_id` without creating storage. Missing definitions, stale
revisions, out-of-range offsets, duplicate coordinates, invalid durable history,
open, replay, append, and serialization failures emit
`recurrence_occurrence_persistence_failed`, return non-zero, and emit no partial
stdout. Failed persistence cannot replace an existing coordinate.

The command reads no ambient time, chooses no due or catch-up policy, generates
no identity, and cannot create schedules or tasks, claim, cancel, dispatch,
retry, grant permission, or execute work. See
[ADR-0046](adr/0046-writable-exact-recurrence-occurrence-provenance-cli.md).

## Read-only exact persisted recurrence occurrence CLI lookup

`vela-dev recurrence occurrence DATABASE RECURRENCE_ID OFFSET` validates the
exact recurrence ID before storage access; clap parses the zero-based offset as
a non-negative `u64` before command execution. It opens only the caller-selected
existing database through `RecurrenceStore::open_read_only` and delegates
canonical coordinate lookup and complete definition/provenance validation to
`RecurrenceStore::load_occurrence`.

Success emits one compact occurrence object preserving exact `recurrence_id`,
`goal`, `offset`, `unix_millis`, and `definition_revision`. Invalid IDs emit
`invalid_recurrence_id`; valid absent coordinates emit
`recurrence_occurrence_not_found`; storage, strict selected-stream replay,
provenance validation, and serialization failures emit
`recurrence_occurrence_lookup_failed`. All failures are non-zero with empty
stdout. Missing storage is not created, and unrelated corruption cannot block
exact lookup.

The command reads no ambient time, persists nothing, enumerates no occurrence
inventory, and cannot choose due or catch-up policy, generate identity,
materialize, claim, cancel, dispatch, retry, grant permission, or execute work.
See [ADR-0047](adr/0047-read-only-exact-persisted-recurrence-occurrence-cli.md).

## Read-only recurrence CLI inventory

`vela-dev recurrence inspect DATABASE` opens the exact caller-selected database
through `RecurrenceStore::open_read_only` and emits one compact JSON document.
Its `recurrences` array retains the kernel's deterministic exact-ID order. Every
object contains exact `id` and `goal` strings plus `anchor_unix_millis`,
`interval_millis`, `occurrence_count`, `final_occurrence_unix_millis`, and
`revision`. An empty compatible store emits `{"recurrences":[]}`; exact strings
are JSON escaped.

Open, schema, replay, projection, and serialization failures emit one escaped
`recurrence_inspection_failed` diagnostic, return non-zero status, and emit no
partial stdout. Inspecting a missing path does not create a database. The
command cannot enumerate occurrences, read ambient time, mutate definitions or
lifecycle state, choose catch-up policy, generate identities, materialize,
dispatch, retry, grant permission, or execute work. See
[ADR-0041](adr/0041-read-only-finite-recurrence-cli-inventory.md).

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

The caller owns every status filter, every cutoff supplied to `list_due`, `claim`, `claim_next_due`, or `materialize_next_due`, every cancellation, claim, exact revision supplied to a mutation, task-ID binding decision, and action taken from a listed, exact, due, claimed, materialized, historical, or task-provenance result. Full, exact, status-filtered, historical, and task-provenance discovery are read-only and grant no authority; `open_read_only` additionally removes SQLite write and creation authority but is not a snapshot, secrecy boundary, or filesystem permission grant. Cancellation prevents future due selection but does not interrupt work already selected elsewhere. A revision identifies one exact persisted observation and prevents an earlier observer or claimant from consuming later lifecycle state; it is not worker identity, a permission grant, a lease, or proof of liveness. A claim is only a durable reservation, release is only caller-authored recovery evidence, and materialization only creates inert active task state: none infers worker health, starts or advances a workflow, calls a provider, invokes a tool, grants or revokes permission, sleeps, retries, or executes task work.

Dispatch, occurrence inventory and lifecycle, catch-up policy, cron syntax, time zones, worker identity, distributed leases, automatic claim expiry, retries, and execution outcomes are intentionally deferred. Finite fixed-interval definitions from [ADR-0037](adr/0037-durable-finite-fixed-interval-recurrence-definitions.md) are inert immutable intent; exact projections and bounded pages remain read-only until callers explicitly persist one coordinate through [ADR-0045](adr/0045-durable-exact-recurrence-occurrence-provenance.md). Persisted provenance grants no catch-up, schedule/task materialization, cancellation, claim, dispatch, or execution authority. A process failure after a successful one-shot claim leaves the schedule claimed until an explicit caller releases or materializes it. See [ADR-0034](adr/0034-durable-one-shot-task-schedule-intent.md) for the one-shot lifecycle decision and rationale.
