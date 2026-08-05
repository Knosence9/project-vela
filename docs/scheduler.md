# Durable task schedules and finite recurrence definitions

The `vela-kernel` crate provides inert scheduler boundaries: callers can persist one-shot task intent, query which one-shot intents are due, persist finite fixed-interval recurrence definitions through the kernel or CLI, inspect those definitions, project exact recurrence occurrences individually, through bounded pages, or through one caller-owned due cutoff, explicitly select the latest due occurrence as a read-only catch-up policy, persist exact occurrence provenance individually, as one atomic bounded due page, or as one atomically selected latest-due coordinate, durably claim and explicitly release one exact persisted due coordinate, atomically materialize either that latest-due selection or one bounded due page as caller-identified inert tasks, inspect persisted provenance, page sparse persisted provenance and materialized task bindings through bounded authored-offset windows, atomically bind one exact available occurrence to a caller-owned inert task through the kernel or CLI, and resolve that exact provenance from the bound task identity. These boundaries record, inspect, select, claim, release, or materialize intent only; they do not read a clock, dispatch work, or execute it.

## Observable contract

- `ScheduleId` is an opaque UTF-8 identity containing at least one non-whitespace character. Exact content is preserved; IDs are not trimmed, case-folded, or normalized. The store isolates owning streams with the internal `schedule:` prefix.
- `ScheduleInstant` is an exact non-negative `u64` count of Unix milliseconds. The kernel does not convert civil times, interpret time zones, or read ambient time.
- `ScheduleInterval` is an exact positive `u64` count of fixed milliseconds. Zero is rejected. `ScheduleInstant::checked_advance` adds one interval deterministically, accepts the maximum exactly representable sum, and returns typed `ScheduleAdvanceError` evidence preserving both operands when the sum would overflow; it never wraps or saturates. `ScheduleInstant::checked_advance_by` derives a caller-owned zero-based occurrence in constant time: offset zero preserves the anchor, offset one is arithmetically equivalent to one-step advancement, and checked multiplication plus checked addition either returns the exact instant (including `u64::MAX`) or typed `ScheduleOccurrenceError` evidence preserving the anchor, interval, and offset. See [ADR-0035](adr/0035-overflow-safe-fixed-interval-schedule-arithmetic.md) and [ADR-0036](adr/0036-indexed-fixed-interval-occurrence-arithmetic.md).
- `RecurrenceId` preserves one exact non-blank UTF-8 identity in the dedicated internal `recurrence:` stream namespace. `OccurrenceCount` is an exact positive `u64`; offsets are zero-based from the immutable anchor and the final offset is `count - 1`.
- `FixedIntervalRecurrence` preserves its exact ID, validated `TaskGoal`, anchor, interval, occurrence count, final occurrence, and persisted revision. `RecurrenceStore::create` validates that final occurrence before appending one version-1 `recurrence.fixed_interval_created` event with `ExpectedVersion::NoStream`. Exact ranges ending at `u64::MAX` succeed; overflow fails with typed operand evidence before persistence. Duplicate creation preserves the original definition. `load` returns no definition for a missing stream and otherwise requires exactly one strictly decoded, fully representable creation event. See [ADR-0037](adr/0037-durable-finite-fixed-interval-recurrence-definitions.md).
- `RecurrenceStore::open_read_only` opens existing recurrence evidence without creating a database or granting SQLite write authority. `list` discovers authoritative recurrence creation streams in one read snapshot, reuses exact history projection, excludes unrelated streams, and returns complete definitions ordered by exact recurrence ID. Empty storage returns an empty list; malformed owning stream IDs, payloads, events, or histories fail closed before any partial inventory is returned. The inventory reads no clock, persists no occurrences or cursors, and grants no lifecycle or execution authority. See [ADR-0040](adr/0040-read-only-finite-recurrence-inventory.md).
- `FixedIntervalRecurrence::occurrence_at(offset)` projects one exact caller-owned zero-based offset in constant time. A successful `RecurrenceOccurrence` preserves the exact recurrence ID, validated goal, offset, instant, and definition revision. Offset zero is the anchor and `count - 1` is the validated final occurrence, including `u64::MAX`; offsets at or above the count return typed evidence preserving the ID, rejected offset, and exact count. Projection alone is not durable occurrence evidence, a claim, or authority to create or execute work; callers must explicitly cross the `persist_occurrence` boundary to record the coordinate. Lookup reads neither storage nor ambient time. See [ADR-0038](adr/0038-exact-finite-recurrence-occurrence-projection.md).
- `FixedIntervalRecurrence::occurrences_page(start_offset, page_size)` projects a deterministic allocation-bounded page in increasing offset order. `OccurrencePageSize` accepts 1 through 1024; zero and larger values fail with typed validation evidence before allocation. The start must identify an authored occurrence, final pages truncate at the finite count, and `next_offset` names the first unreturned offset only when another page remains. Bound arithmetic cannot wrap, and every item reuses exact lookup to preserve full provenance. The cursor is read-only projection coordinates, not durable state, due or catch-up policy, or execution authority. See [ADR-0039](adr/0039-bounded-finite-recurrence-occurrence-paging.md).
- `RecurrenceStore::due_occurrences_page(id, start_offset, page_size, cutoff)` strictly loads one exact immutable definition and projects complete occurrences in ascending offset order only while their deterministic instants are at or before the inclusive caller-owned cutoff. Work and allocation remain bounded by `OccurrencePageSize`. `next_offset` advances after a full page, points at the first future authored coordinate when the cutoff stops selection, and is absent only at the finite definition end, allowing a later cutoff to resume without rescanning. Missing definitions, invalid starts, and malformed selected definition evidence preserve existing typed failures. The operation reads no ambient clock, persists no cursor, and grants no global discovery, catch-up choice, generated identity, lifecycle, dispatch, retry, or execution authority. See [ADR-0056](adr/0056-caller-cutoff-finite-recurrence-due-paging.md).
- `RecurrenceStore::latest_due_occurrence(id, start_offset, cutoff)` makes one latest-only catch-up choice explicit while retaining caller ownership of the authored start and inclusive cutoff. It strictly loads one exact immutable definition, validates the start through exact projection, and derives the latest due offset with constant-space arithmetic rather than enumerating skipped backlog. `LatestDueOccurrenceSelection` contains the complete selected occurrence and its following authored offset, or finite completion; a future starting coordinate returns no occurrence and preserves that unchanged cursor. Missing definitions and invalid starts retain existing typed failures. The operation reads no ambient clock, persists no cursor or skip evidence, and grants no global discovery, occurrence persistence, materialization, task lifecycle, dispatch, retry, permission, or execution authority. See [ADR-0060](adr/0060-explicit-latest-only-recurrence-catch-up-selection.md).
- `RecurrenceStore::persist_latest_due_occurrence(id, expected_revision, start_offset, cutoff)` reuses the exact constant-space latest-only projection, then atomically appends canonical provenance for only the selected coordinate while rechecking the immutable definition revision and selected-stream absence. A future horizon returns no occurrence with its unchanged cursor and writes nothing; finite completion remains explicit. Existing selected provenance is strictly validated and rejected, selected corruption fails closed, and skipped coordinates are neither inspected nor persisted. The returned cursor and skipped work are not durable acceptance, waiver, or lifecycle evidence. The operation reads no ambient clock, discovers no unrelated recurrence, generates no identity, and grants no materialization, task lifecycle, dispatch, retry, permission, or execution authority. See [ADR-0062](adr/0062-atomic-latest-due-recurrence-provenance-persistence.md).
- `RecurrenceStore::materialize_latest_due_occurrence(id, expected_revision, start_offset, cutoff, task_id)` reuses the exact latest-only projection and atomically creates the selected coordinate's canonical persisted revision, materialization revision, and caller-identified `task.started` stream while rechecking the recurrence revision and both stream absences. Success returns the complete materialized binding and following authored offset or finite completion. A future horizon returns no binding with its unchanged cursor and writes nothing. Existing selected provenance is rejected rather than consumed; exact `materialize_occurrence` remains the recovery boundary for persisted-only evidence. Skipped coordinates are uninspected and unpersisted. The operation reads no clock, persists no cursor or skip evidence, generates no identity, and grants no claim, dispatch, retry, permission, or execution authority. See [ADR-0064](adr/0064-atomic-latest-due-recurrence-task-materialization.md).
- `RecurrenceStore::persist_due_occurrences_page(id, expected_revision, start_offset, page_size, cutoff)` validates one exact immutable definition revision, reuses the bounded inclusive-cutoff selection above, and atomically appends canonical provenance for every selected coordinate. Every selected stream must be absent; an existing coordinate is strictly validated and rejected with typed `OccurrenceAlreadyPersisted` evidence rather than skipped. The recurrence prerequisite and all selected streams are rechecked in one immediate transaction, so success persists the complete page while every stale, duplicate, corruption, serialization, storage, or competing-write failure persists none. An empty future-horizon page succeeds without writes and preserves its unchanged resumable cursor. The operation reads no ambient clock, persists no cursor, and grants no global discovery, catch-up policy, generated identity, materialization, task lifecycle, dispatch, retry, permission, or execution authority. See [ADR-0058](adr/0058-atomic-bounded-due-recurrence-provenance-persistence.md).
- `RecurrenceStore::materialize_due_occurrences_page(id, expected_revision, start_offset, page_size, cutoff, task_ids)` reuses bounded due paging and requires exactly one distinct caller-owned task ID per selected coordinate in authored-offset order. One immediate transaction rechecks the immutable recurrence revision plus every selected occurrence and task stream, then appends each coordinate's canonical persisted and materialized events with its authoritative-goal `task.started` event. Success returns complete revision-2 bindings and the unchanged due-page cursor. Typed count mismatch, duplicate identity, existing provenance, task collision, corruption, storage failure, or contention leaves the whole page absent; a future empty page requires no IDs and writes nothing. Existing exact materialization remains the available-provenance recovery boundary. The operation reads no clock, generates no identity, persists no cursor, and grants no discovery, catch-up, claim, dispatch, retry, permission, or execution authority. See [ADR-0066](adr/0066-atomic-bounded-due-recurrence-task-materialization.md).
- `RecurrenceStore::persist_occurrence(id, expected_revision, offset)` records one exact authored coordinate as version-1 `recurrence.occurrence_persisted` evidence after exact definition replay, revision validation, and authoritative projection. The canonical internal stream identity is a collision-free byte-length-prefixed recurrence ID plus offset; duplicates return typed evidence and never replace the first event. `load_occurrence` replays only that coordinate and requires its exact ID, definition revision, offset, goal, and instant to agree with the authoritative definition. Missing coordinates return `None`; malformed, unsupported, divergent, or invalid lifecycle histories fail closed. Persistence reads no clock and grants no catch-up, schedule/task materialization, claim, dispatch, or execution authority. See [ADR-0045](adr/0045-durable-exact-recurrence-occurrence-provenance.md).
- `RecurrenceStore::claim_occurrence(id, offset, expected_occurrence_revision, cutoff)` durably reserves one exact available coordinate by appending version-1 `recurrence.occurrence_claimed` at its exact observed revision only when its authoritative instant is at or before the inclusive caller-owned cutoff. Success returns complete `ClaimedRecurrenceOccurrence` provenance; `load_claimed_occurrence` exposes only the current exact reservation after reopen. Revision validation precedes lifecycle and due checks, and racing claims commit exactly one event. Existing persisted provenance views include claimed coordinates, while materialized views exclude them and direct exact materialization rejects current claims. The claim reads no clock and grants no worker identity, lease, permission, dispatch, retry, or execution authority. See [ADR-0068](adr/0068-exact-persisted-recurrence-occurrence-claiming.md).
- `RecurrenceStore::release_occurrence(id, offset, expected_occurrence_revision, reason)` returns one exact claimed coordinate to available persisted state by appending version-1 `recurrence.occurrence_released` with a validated non-blank caller-authored reason. Revision validation precedes lifecycle validation. Success returns complete `ReleasedRecurrenceOccurrence` provenance with the resulting revision and exact latest recovery reason; `load_released_occurrence` exposes that evidence only while release remains the current available state. A released coordinate can be claimed again or directly materialized at its exact revision. Strict replay accepts `persisted -> (claimed -> released)*` followed by an optional final claim or materialization; impossible ordering and malformed recovery evidence fail closed. Missing, stale, available, materialized, read-only, and competing transitions append nothing. Release reads no clock, infers no worker failure, and grants no lease, permission, dispatch, retry, or execution authority. See [ADR-0069](adr/0069-exact-recurrence-occurrence-claim-release.md).
- `RecurrenceStore::materialize_claimed_occurrence(id, offset, expected_occurrence_revision, task_id)` atomically consumes one exact current claim into caller-identified inert task state. It validates the observed occurrence revision before lifecycle state, then appends `recurrence.occurrence_materialized` and authoritative-goal `task.started` events as one transaction. Task collisions preserve the claim; occurrence contention creates no orphan task. Strict replay accepts claimed materialization after any complete claim/release cycles as terminal, while direct `materialize_occurrence` remains limited to available persisted or released state. The operation needs no cutoff because the claim recorded due authority, and grants no inventory, claim-next, worker, lease, dispatch, retry, permission, or execution authority. See [ADR-0072](adr/0072-exact-claimed-recurrence-occurrence-task-materialization.md).
- `RecurrenceStore::persisted_occurrences_page(id, start_offset, page_size)` strictly loads one exact definition, inspects at most 1024 authored coordinates in ascending offset order, and returns only coordinates with valid durable provenance. Missing coordinates are omitted, including valid all-gap pages. `next_offset` identifies the first uninspected authored offset or is absent after the finite end, so cursor progress is independent of result density. The selected definition is replayed once; every present coordinate reuses canonical exact-stream replay and authoritative validation. Selected-window corruption fails closed before partial output, while unrelated definitions and coordinates outside the window cannot block inspection. The page reads no clock, persists nothing, and grants no catch-up, materialization, claim, dispatch, or execution authority. See [ADR-0048](adr/0048-bounded-persisted-recurrence-occurrence-paging.md).
- `RecurrenceStore::materialized_occurrences_page(id, start_offset, page_size)` reuses the same exact-definition authored-window and `OccurrencePageSize` bound, but returns only complete task-bound `MaterializedRecurrenceOccurrence` values in increasing offset order. Missing, persisted-only, released, and claimed coordinates are omitted, including valid empty pages, while `next_offset` advances by inspected authored coordinates and truncates at the finite end. The selected definition is replayed once and every existing selected coordinate is strictly replayed and validated, so selected-window corruption fails closed without letting unrelated or out-of-window corruption block inspection. The page works through read-only storage, mutates nothing, and grants no global discovery, catch-up, lifecycle, dispatch, or execution authority. See [ADR-0054](adr/0054-bounded-materialized-recurrence-occurrence-paging.md).
- `RecurrenceStore::materialize_occurrence(id, offset, expected_occurrence_revision, task_id)` requires one exact strictly validated available coordinate, either persisted-only at revision 1 or explicitly released at its later exact revision. It atomically appends `recurrence.occurrence_materialized` at that exact revision and the existing `task.started` event with `ExpectedVersion::NoStream`; the task receives the occurrence's authoritative goal. The resulting materialized projection preserves the exact occurrence, resulting occurrence revision, and caller-owned task ID. Missing provenance, stale revisions, current claimed state, task collisions, duplicate or replacement bindings, malformed lifecycle histories, and competing transitions leave both streams unchanged. Existing `load_occurrence` and sparse persisted paging remain provenance views over every valid lifecycle; claimed, released, and materialized exact lookups expose only their respective current states. The boundary reads no clock, generates no identity, scans no unrelated coordinates, and grants no catch-up, selection, dispatch, retry, permission, or execution authority. See [ADR-0050](adr/0050-atomic-exact-recurrence-occurrence-task-materialization.md) and [ADR-0069](adr/0069-exact-recurrence-occurrence-claim-release.md).
- `RecurrenceStore::find_materialized_by_task_id(task_id)` resolves complete recurrence-occurrence provenance from one exact caller-owned task identity. It selects only materialization markers carrying that exact identity, strictly replays every selected canonical occurrence stream, and validates each coordinate against its immutable recurrence definition. An unrelated task returns `None`; multiple corrupted bindings return typed `AmbiguousTaskBinding` evidence rather than an arbitrary result. Malformed selected stream IDs, payloads, histories, missing definitions, and divergent provenance fail closed, while invalid JSON in unrelated materialization markers cannot block the exact query. Lookup mutates nothing and grants no lifecycle, catch-up, dispatch, or execution authority. See [ADR-0052](adr/0052-read-only-materialized-recurrence-task-provenance.md).
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

## Read-only due recurrence occurrence CLI paging

`vela-dev recurrence due DATABASE RECURRENCE_ID START_OFFSET PAGE_SIZE
CUTOFF_UNIX_MILLIS` validates the exact recurrence ID and positive, at-most-1024
page size before storage access; clap parses the authored offset and explicit
Unix-millisecond cutoff as non-negative `u64` values. It opens only the selected
existing database through `RecurrenceStore::open_read_only` and delegates the
bounded inclusive-cutoff projection to `RecurrenceStore::due_occurrences_page`.

Success emits complete occurrences in ascending authored-offset order, preserving
exact `recurrence_id`, `goal`, `offset`, `unix_millis`, and
`definition_revision`. `next_offset` advances after a full page, points at the
first future authored coordinate when the cutoff stops selection, and is `null`
only at the finite definition end. An empty page with a non-null unchanged cursor
therefore records a temporary cutoff horizon that a later caller-owned cutoff can
resume without rescanning.

Invalid IDs and page sizes emit `invalid_recurrence_id` and
`invalid_occurrence_page_size` before storage access. Missing definitions emit
`recurrence_not_found`; invalid starts emit
`recurrence_occurrence_out_of_range`. Open, strict selected-definition replay,
projection, and serialization failures emit
`due_recurrence_occurrence_lookup_failed`, return non-zero, and emit no stdout.
Missing storage remains missing, and unrelated corruption cannot block the exact
query.

The command reads no ambient clock, persists no cursor, and grants no global
discovery, catch-up choice, generated identity, occurrence persistence,
materialization, claim, dispatch, workflow, provider/tool, permission, retry, or
execution authority. See
[ADR-0057](adr/0057-read-only-due-recurrence-occurrence-cli-paging.md).

## Read-only latest-due recurrence occurrence CLI selection

`vela-dev recurrence latest-due DATABASE RECURRENCE_ID START_OFFSET
CUTOFF_UNIX_MILLIS` validates the exact recurrence ID before storage access;
clap parses the authored start and inclusive caller-owned cutoff as non-negative
`u64` values. It opens only the selected existing database through
`RecurrenceStore::open_read_only` and delegates selection to
`RecurrenceStore::latest_due_occurrence` without enumerating skipped backlog.

Success emits compact JSON with `occurrence` containing the complete selected
`recurrence_id`, `goal`, `offset`, `unix_millis`, and `definition_revision`, or
`null` when the starting coordinate remains future. `next_offset` is the
kernel-owned following authored coordinate, the unchanged future cursor, or
`null` at finite completion. Exact caller-authored strings remain JSON escaped.

Invalid IDs emit `invalid_recurrence_id` before storage access. Missing
definitions emit `recurrence_not_found`; invalid starts emit
`recurrence_occurrence_out_of_range`. Other open, strict selected-definition
replay, projection, and serialization failures emit
`latest_due_recurrence_occurrence_lookup_failed`, return non-zero, and emit no
stdout. Missing storage remains missing, and unrelated corruption cannot block
the exact query.

The command reads no ambient clock, persists no cursor or skip evidence,
discovers no unrelated definition, generates no identity, and grants no
occurrence persistence, materialization, task lifecycle, dispatch, workflow,
provider/tool, permission, retry, or execution authority. See
[ADR-0061](adr/0061-read-only-latest-due-recurrence-cli.md).

## Writable atomic latest-due recurrence occurrence CLI persistence

`vela-dev recurrence persist-latest-due DATABASE RECURRENCE_ID
EXPECTED_REVISION START_OFFSET CUTOFF_UNIX_MILLIS` validates the exact
recurrence ID before storage access; clap parses the observed definition
revision, authored start, and inclusive caller-owned cutoff as non-negative
`u64` values. It opens only the selected database through
`RecurrenceStore::open` and delegates constant-space selection, revision
validation, duplicate protection, and atomic persistence to
`RecurrenceStore::persist_latest_due_occurrence`.

Success emits the same compact shape as read-only latest-due selection.
`occurrence` contains complete persisted provenance or `null`; `next_offset`
contains the following authored coordinate, the unchanged future cursor, or
`null` at finite completion. A future horizon writes nothing, and skipped
coordinates remain uninspected and unpersisted.

Invalid IDs emit `invalid_recurrence_id` before storage access. Missing or stale
definitions, invalid starts, duplicates, selected corruption, concurrency,
open, persistence, and serialization failures emit
`latest_due_recurrence_occurrence_persistence_failed`, return non-zero, and
emit no stdout. Every failure appends no selected provenance.

The command reads no ambient clock, persists no cursor or skipped-coordinate
evidence, discovers no unrelated recurrence, generates no identity, and grants
no materialization, task lifecycle, claim, dispatch, workflow, provider/tool,
permission, retry, or execution authority. See
[ADR-0063](adr/0063-writable-atomic-latest-due-recurrence-cli.md).

## Writable atomic latest-due recurrence task materialization CLI

`vela-dev recurrence materialize-latest-due DATABASE RECURRENCE_ID
EXPECTED_REVISION START_OFFSET CUTOFF_UNIX_MILLIS TASK_ID` validates both exact
identities before storage access; clap parses the observed definition revision,
authored start, and inclusive caller-owned cutoff as non-negative `u64` values.
It opens only the selected database through `RecurrenceStore::open` and delegates
constant-space selection, revision validation, selected occurrence and task
uniqueness, and atomic materialization to
`RecurrenceStore::materialize_latest_due_occurrence`.

Success emits compact JSON with `occurrence` containing the complete materialized
binding or `null`, and `next_offset` containing the following authored coordinate,
the unchanged future cursor, or `null` at finite completion. A future horizon
writes nothing. Skipped coordinates remain uninspected and unpersisted.

Invalid identities emit `invalid_recurrence_id` or `invalid_task_id` before
storage access. Missing or stale definitions, invalid starts, existing or
malformed selected provenance, task collisions, concurrency, open, replay,
append, and serialization failures emit
`latest_due_recurrence_occurrence_materialization_failed`, return non-zero, and
emit no stdout. Every failure leaves no partial occurrence history or orphan task.

The command reads no ambient clock, persists no cursor or skipped-coordinate
evidence, discovers no unrelated recurrence, generates no identity, and grants
no claim, lease, dispatch, workflow, provider/tool, permission, retry, or
execution authority. See
[ADR-0065](adr/0065-writable-atomic-latest-due-recurrence-task-materialization-cli.md).

## Writable atomic due recurrence occurrence CLI paging

`vela-dev recurrence persist-due DATABASE RECURRENCE_ID EXPECTED_REVISION
START_OFFSET PAGE_SIZE CUTOFF_UNIX_MILLIS` validates the exact recurrence ID and
positive, at-most-1024 page size before storage access; clap parses the observed
definition revision, authored offset, and caller-owned cutoff as non-negative
`u64` values. It opens only the selected database through
`RecurrenceStore::open` and delegates selection, revision validation, duplicate
protection, and atomic persistence to
`RecurrenceStore::persist_due_occurrences_page`.

Success emits complete persisted occurrences in ascending authored-offset order.
Every object preserves exact `recurrence_id`, `goal`, `offset`, `unix_millis`,
and `definition_revision`. `next_offset` advances after a full page, identifies
the first future authored coordinate when cutoff truncates selection, preserves
an unchanged cursor for an empty future-horizon page, and is `null` only at the
finite definition end.

Invalid IDs and page sizes emit `invalid_recurrence_id` and
`invalid_occurrence_page_size` without accessing storage. Missing definitions,
stale revisions, out-of-range starts, duplicates, selected corruption,
concurrency, open, persistence, and serialization failures emit
`due_recurrence_occurrence_persistence_failed`, return non-zero, and emit no
stdout. Every failure persists none of the selected page.

The command reads no ambient clock, persists no cursor, generates no identity,
and grants no global discovery, catch-up policy, materialization, task lifecycle,
claim, dispatch, workflow, provider/tool, permission, retry, or execution
authority. See
[ADR-0059](adr/0059-writable-atomic-due-recurrence-page-cli.md).

## Writable atomic due recurrence task materialization CLI

`vela-dev recurrence materialize-due DATABASE RECURRENCE_ID EXPECTED_REVISION
START_OFFSET PAGE_SIZE CUTOFF_UNIX_MILLIS [TASK_IDS]...` validates the exact
recurrence ID, positive at-most-1024 page size, and every supplied task identity
before storage access; clap parses all numeric coordinates as non-negative
`u64` values. It opens only the selected database through
`RecurrenceStore::open` and delegates bounded due selection, ordered task-count
and duplicate validation, exact revision and stream checks, and atomic page
materialization to `RecurrenceStore::materialize_due_occurrences_page`.

Success emits complete materialized bindings in authored-offset order. Every
object preserves exact `recurrence_id`, `goal`, `offset`, `unix_millis`,
`definition_revision`, resulting `occurrence_revision`, and `task_id`;
`next_offset` preserves the kernel's resumable due-page semantics. An empty
future-horizon page accepts zero task IDs, emits an empty array with its
unchanged cursor, and writes nothing. Every non-empty selection requires exactly
one ordered task ID per occurrence.

Invalid IDs and page sizes emit `invalid_recurrence_id`,
`invalid_occurrence_page_size`, or `invalid_task_id` without accessing storage.
Missing or stale definitions, invalid starts, count mismatch, duplicate task
IDs, selected provenance or corruption, task collisions, concurrency, open,
replay, append, and serialization failures emit
`due_recurrence_occurrence_materialization_failed`, return non-zero, and emit no
stdout. Kernel mutation failures leave no selected prefix or orphan task.
Serialization follows a successful atomic commit and cannot roll it back; the
current fixed-field projection has no data-dependent serialization failure.

The command reads no ambient clock, persists no cursor, discovers no unrelated
recurrence, generates no identity, and grants no catch-up policy, claim, lease,
dispatch, workflow, provider/tool, permission, retry, or execution authority.
See [ADR-0067](adr/0067-writable-atomic-due-recurrence-task-materialization-cli.md).

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

## Writable exact recurrence occurrence claim CLI

`vela-dev recurrence claim DATABASE RECURRENCE_ID OFFSET
EXPECTED_OCCURRENCE_REVISION CUTOFF_UNIX_MILLIS` validates the exact recurrence
ID before storage access; clap parses the coordinate, observed occurrence
revision, and caller-owned inclusive cutoff as non-negative `u64` values. It
opens only the caller-selected database through `RecurrenceStore::open` and
delegates strict replay, exact concurrency and lifecycle checks, due validation,
and append to `RecurrenceStore::claim_occurrence`.

Success emits one compact object preserving exact `recurrence_id`, `goal`,
`offset`, `unix_millis`, `definition_revision`, and resulting
`occurrence_revision`. Invalid IDs emit `invalid_recurrence_id` before storage
access. Missing provenance, stale or unavailable lifecycle state, future
instants, corruption, contention, open, replay, append, and serialization
failures emit `recurrence_occurrence_claim_failed`, return non-zero, and emit no
stdout. Rejected claims do not alter occurrence lifecycle state.

The command reads no ambient clock, scans no unrelated coordinate, generates no
identity, and grants no materialization, release, worker, lease, dispatch,
retry, permission, provider/tool, workflow, or execution authority. See
[ADR-0070](adr/0070-writable-exact-recurrence-occurrence-claim-cli.md).

## Writable exact recurrence occurrence release CLI

`vela-dev recurrence release DATABASE RECURRENCE_ID OFFSET
EXPECTED_OCCURRENCE_REVISION REASON` validates the exact recurrence ID and one
non-blank caller-authored recovery reason before storage access; clap parses the
coordinate and observed occurrence revision as non-negative `u64` values. It
opens only the caller-selected database through `RecurrenceStore::open` and
delegates strict replay, revision-before-lifecycle validation, contention, and
append to `RecurrenceStore::release_occurrence`.

Success emits one compact object preserving exact `recurrence_id`, `goal`,
`offset`, `unix_millis`, `definition_revision`, resulting
`occurrence_revision`, and `latest_release`. Invalid IDs and reasons emit
`invalid_recurrence_id` and `invalid_recurrence_occurrence_release` before
storage access. Missing, stale, available, materialized, corrupt, contended,
read-only, open, replay, append, and serialization failures emit
`recurrence_occurrence_release_failed`, return non-zero, and emit no stdout.
Validation, storage, and transition rejection append no release evidence.
Response serialization follows a successful durable append; its fixed
string-and-integer projection is infallible with the selected serializer, but a
future serialization failure would report the already-durable release rather
than roll it back.

The reason records explicit recovery evidence only. The command scans no
unrelated coordinate, reads no ambient clock, infers no worker death, expires no
lease, and grants no worker identity, dispatch, retry, permission,
provider/tool, workflow, or execution authority. Released provenance can be
reclaimed or directly materialized only through a later exact-revision command.
See [ADR-0071](adr/0071-writable-exact-recurrence-occurrence-release-cli.md).

## Writable exact recurrence occurrence materialization CLI

`vela-dev recurrence materialize DATABASE RECURRENCE_ID OFFSET
EXPECTED_OCCURRENCE_REVISION TASK_ID` validates both exact caller-owned
identities before storage access; clap parses the zero-based offset and observed
occurrence revision as non-negative `u64` values before command execution. It
opens only the caller-selected database through `RecurrenceStore::open` and
delegates strict replay, exact optimistic concurrency, lifecycle validation,
task-stream uniqueness, and the atomic append to
`RecurrenceStore::materialize_occurrence`.

Success emits one compact object preserving exact `recurrence_id`, `goal`,
`offset`, `unix_millis`, `definition_revision`, resulting
`occurrence_revision`, and `task_id`. Invalid identities emit
`invalid_recurrence_id` or `invalid_task_id` without creating storage. Missing,
stale, already-materialized, or task-colliding inputs, plus open, replay,
append, and serialization failures, emit
`recurrence_occurrence_materialization_failed`, return non-zero, and emit no
partial stdout. Every failed operation leaves both streams unchanged.

The command reads no clock, generates no identity, scans no unrelated
coordinate, and grants no catch-up, selection, claim, lease, dispatch, retry,
permission, provider/tool, workflow, or execution authority. See
[ADR-0051](adr/0051-writable-exact-recurrence-occurrence-task-materialization-cli.md).

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

## Read-only persisted recurrence occurrence CLI paging

`vela-dev recurrence persisted DATABASE RECURRENCE_ID START_OFFSET PAGE_SIZE`
validates the exact recurrence ID and positive, at-most-1024 page size before
storage access. It opens only the caller-selected existing database through
`RecurrenceStore::open_read_only` and delegates sparse authored-window paging to
`RecurrenceStore::persisted_occurrences_page`.

Success emits compact JSON with durable `occurrences` in ascending offset order
and `next_offset` naming the first uninspected authored coordinate or `null` at
the finite end. Missing coordinates are omitted, so a valid all-gap window emits
an empty array with an advancing cursor. Every returned object preserves exact
`recurrence_id`, `goal`, `offset`, `unix_millis`, and `definition_revision`.

Invalid IDs and page sizes emit `invalid_recurrence_id` and
`invalid_occurrence_page_size` before storage access. Missing definitions emit
`recurrence_not_found`; invalid starts emit
`recurrence_occurrence_out_of_range`. Open, replay, selected-window corruption,
paging, and serialization failures emit
`persisted_recurrence_occurrence_lookup_failed`. All failures are non-zero with
empty stdout, missing storage is not created, and unrelated or out-of-window
corruption cannot block the selected page.

The command reads no ambient time, persists nothing, performs no global
inventory, and cannot choose catch-up policy, generate identity, materialize,
claim, cancel, dispatch, retry, grant permission, or execute work. See
[ADR-0049](adr/0049-read-only-persisted-recurrence-occurrence-cli-paging.md).

## Read-only materialized recurrence occurrence CLI paging

`vela-dev recurrence materialized DATABASE RECURRENCE_ID START_OFFSET PAGE_SIZE`
validates the exact recurrence ID and positive, at-most-1024 page size before
storage access. It opens only the selected existing database through
`RecurrenceStore::open_read_only` and delegates the bounded authored-offset
window to `RecurrenceStore::materialized_occurrences_page`.

Success emits complete materialized bindings in ascending offset order. Each
object preserves exact `recurrence_id`, `goal`, `offset`, `unix_millis`,
`definition_revision`, `occurrence_revision`, and `task_id`. Missing and
persisted-only coordinates are omitted; `next_offset` still advances by every
inspected authored coordinate and is `null` at the finite definition end.

Invalid IDs and page sizes emit `invalid_recurrence_id` and
`invalid_occurrence_page_size` before storage access. Missing definitions emit
`recurrence_not_found`; invalid starts emit
`recurrence_occurrence_out_of_range`. Open, strict selected-window replay,
provenance, paging, and serialization failures emit
`materialized_recurrence_occurrence_lookup_failed` with non-zero status and no
stdout. Missing storage remains missing, and unrelated or out-of-window
corruption cannot block the selected page.

The command reads no clock, mutates nothing, persists no cursor, and grants no
global discovery, catch-up, due-selection, identity generation, lifecycle,
claim, dispatch, workflow, provider/tool, permission, retry, or execution
authority. See
[ADR-0055](adr/0055-read-only-materialized-recurrence-occurrence-cli-paging.md).

## Read-only recurrence task-provenance CLI

`vela-dev recurrence task DATABASE TASK_ID` validates the exact caller-owned
task identity before storage access, opens only the selected existing database
through `RecurrenceStore::open_read_only`, and delegates strict selected-stream
replay, canonical coordinate recovery, definition validation, and ambiguity
detection to `RecurrenceStore::find_materialized_by_task_id`.

Success emits one compact JSON object containing exact `task_id` and
`occurrence`. A bound result uses the complete materialized occurrence shape:
`recurrence_id`, `goal`, `offset`, `unix_millis`, `definition_revision`,
`occurrence_revision`, and `task_id`. A valid unbound identity emits
`"occurrence":null`. Every caller-authored string is JSON escaped.

Invalid identities emit `invalid_task_id` before storage access. Open, replay,
ambiguity, provenance, and serialization failures emit
`recurrence_task_lookup_failed`, return non-zero, and emit no stdout. Missing
storage is never created. The command reads no clock, mutates nothing,
enumerates no unrelated occurrence, and grants no catch-up, due-selection,
dispatch, workflow, provider/tool, permission, retry, or execution authority.
See [ADR-0053](adr/0053-read-only-recurrence-task-provenance-cli.md).

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

The caller owns every status filter, every cutoff supplied to `list_due`, `claim`, `claim_next_due`, `materialize_next_due`, recurrence due paging and persistence, or latest-only recurrence selection, every cancellation, claim, exact revision supplied to a mutation, task-ID binding decision, and action taken from a listed, exact, due, latest-selected, claimed, materialized, historical, or task-provenance result. Full, exact, status-filtered, historical, and task-provenance discovery are read-only and grant no authority; `open_read_only` additionally removes SQLite write and creation authority but is not a snapshot, secrecy boundary, or filesystem permission grant. Cancellation prevents future due selection but does not interrupt work already selected elsewhere. A revision identifies one exact persisted observation and prevents an earlier observer or claimant from consuming later lifecycle state; it is not worker identity, a permission grant, a lease, or proof of liveness. A claim is only a durable reservation, release is only caller-authored recovery evidence, and materialization only creates inert active task state: none infers worker health, starts or advances a workflow, calls a provider, invokes a tool, grants or revokes permission, sleeps, retries, or executes task work. Exact recurrence task-provenance lookup selects only the caller-owned task identity, validates every selected occurrence binding, and treats duplicate bindings as corruption; it grants no lifecycle or execution authority.

Dispatch, global occurrence inventory, claimed occurrence inventory and claim-next selection, additional catch-up policies, durable skip evidence, cron syntax, time zones, worker identity, distributed leases, automatic claim expiry, retries, and execution outcomes are intentionally deferred. Finite fixed-interval definitions from [ADR-0037](adr/0037-durable-finite-fixed-interval-recurrence-definitions.md) are inert immutable intent; exact projections, bounded pages, and latest-only catch-up selection remain read-only until callers explicitly persist one coordinate through [ADR-0045](adr/0045-durable-exact-recurrence-occurrence-provenance.md). Persisted provenance alone grants no catch-up, selection, cancellation, claim, dispatch, or execution authority; an explicit exact-revision call may claim available provenance, release a claim with recovery evidence, atomically bind available provenance to one inert caller-owned task, or atomically consume one current claim into one inert caller-owned task. A process failure after a successful claim leaves a one-shot schedule or recurrence occurrence claimed until an explicit caller-owned release or claimed-consumption transition. Direct recurrence `materialize_occurrence` remains limited to available persisted or released coordinates; `materialize_claimed_occurrence` is the separate exact-revision claimed-consumption boundary. See [ADR-0034](adr/0034-durable-one-shot-task-schedule-intent.md), [ADR-0050](adr/0050-atomic-exact-recurrence-occurrence-task-materialization.md), [ADR-0069](adr/0069-exact-recurrence-occurrence-claim-release.md), and [ADR-0072](adr/0072-exact-claimed-recurrence-occurrence-task-materialization.md).
