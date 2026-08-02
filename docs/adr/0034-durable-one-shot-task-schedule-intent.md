# ADR-0034: Durable inert one-shot task schedule intent

- **Status:** accepted
- **Date:** 2026-07-31
- **Decision and execution issues:** [#789](https://github.com/Knosence9/project-vela/issues/789), [#791](https://github.com/Knosence9/project-vela/issues/791), [#793](https://github.com/Knosence9/project-vela/issues/793), [#795](https://github.com/Knosence9/project-vela/issues/795), [#797](https://github.com/Knosence9/project-vela/issues/797), [#799](https://github.com/Knosence9/project-vela/issues/799), [#803](https://github.com/Knosence9/project-vela/issues/803), [#805](https://github.com/Knosence9/project-vela/issues/805), [#807](https://github.com/Knosence9/project-vela/issues/807), [#811](https://github.com/Knosence9/project-vela/issues/811), [#813](https://github.com/Knosence9/project-vela/issues/813), [#841](https://github.com/Knosence9/project-vela/issues/841), [#843](https://github.com/Knosence9/project-vela/issues/843), [#847](https://github.com/Knosence9/project-vela/issues/847)

## Context

Vela has durable task, workflow, and independent Verification lifecycles, but no code-owned scheduling primitive. The North Star assigns deterministic scheduling to code, and the architecture reserves scheduler/cron behavior for the inspectable kernel. Reading an ambient clock and dispatching work in the first scheduler operation would combine time authority, persistence, execution, retry, and permission decisions before their failure semantics exist.

The typed SQLite event log already provides exact stream identity, immutable append, replay, and snapshot-consistent discovery. A smaller first boundary can persist inert one-shot intent and deterministically report which intents are due relative to caller-owned input.

## Decision

`ScheduleStore` persists one immutable `schedule.created` event at payload version `1` for each exact `ScheduleId`. The event contains a validated `TaskGoal` and a `ScheduleInstant`, represented as an exact non-negative count of Unix milliseconds. Schedule IDs must contain at least one non-whitespace character; otherwise their exact UTF-8 is preserved and compared without normalization.

`ScheduleStore::schedule` uses `ExpectedVersion::NoStream`, so an exact ID can be created once and a duplicate cannot replace its original goal or due instant. `ScheduleStore::cancel(id, expected_revision, reason)` appends one version-one `schedule.cancelled` event with an exact non-blank caller reason only while that exact revision remains pending. Missing, invalid-state, and stale schedules return typed errors without rewriting history. `load` replays one exact stream and projects explicit `Pending`, `Cancelled`, `Claimed`, or `Materialized` status while preserving cancellation, latest release, and task-binding evidence. Impossible lifecycle histories fail closed.

`ScheduleStore::list()` discovers streams from their authoritative creation events in one SQLite read snapshot and returns every pending, cancelled, claimed, and materialized intent ordered by exact schedule ID. `ScheduleStore::list_by_status(status)` reuses the same read-only discovery boundary and returns only intents whose explicit persisted status exactly matches the caller-owned filter, preserving exact schedule-ID order and returning an empty list when no intent matches. `ScheduleStore::list_due(cutoff)` reuses discovery and returns every pending intent whose due instant is less than or equal to the caller-owned cutoff, ordered by due instant and then exact schedule ID. Cancelled, claimed, and non-schedule streams are excluded from due results. Invalid owning stream IDs, malformed payloads, unsupported events, and impossible lifecycle histories are errors rather than partial results from these queries.

`ScheduleStore::claim(id, expected_revision, cutoff)` durably reserves one due intent by appending an empty `schedule.claimed` event only while that exact revision remains pending and its due instant is at or before the caller-owned cutoff. A future schedule returns typed `NotDue` evidence without an append. Cancellation and claiming are competing transitions from pending: exactly one can win a race, and the loser receives typed `ConcurrentModification` evidence. Exact-revision validation occurs before lifecycle-state and due-cutoff validation, so an earlier pending observer cannot act after an intervening claim/release cycle. Claimed schedules survive reopen and are excluded from due discovery.

`ScheduleStore::claim_next_due(cutoff)` centralizes deterministic worker selection without adding clock or dispatch authority. It selects the earliest pending schedule in the same due-instant then exact-ID order, attempts its exact-revision claim, and returns the complete claimed projection. When another persisted transition consumes that revision first, it restarts validated selection rather than exposing the internal optimistic conflict; therefore concurrent callers reserve distinct eligible schedules while work remains. No eligible work returns `None`. Complete discovery remains fail-closed, so corruption is never skipped to claim a later schedule. Each retry is justified by a persisted competing transition and performs no sleeping or execution.

`vela-dev schedule claim-next DATABASE CUTOFF_UNIX_MILLIS` exposes that exact claim-next boundary to automation without duplicating selection policy. It emits the complete claimed projection under a `schedule` key, or `null` when no eligible work remains; failures emit no partial JSON. The CLI supplies only the caller-owned database and cutoff and gains no ambient clock, worker identity, task-ID generation, dispatch, materialization, or execution authority.

`ScheduleStore::materialize_next_due(cutoff, task_id)` provides a separate atomic consumption boundary for callers that already own the task identity and do not need a recoverable claim gap. It selects the earliest pending due schedule in the same deterministic order, then atomically appends `schedule.materialized` directly against that exact pending revision and the existing `task.started` event against `NoStream`. A competing schedule transition restarts selection; a task-ID collision consumes no schedule and is returned without generating another identity. No due work returns `None`. Direct pending materialization and the existing explicit `claimed -> materialized` path are both valid histories. The operation remains inert: it reads no clock and dispatches or executes no work. CLI exposure is intentionally deferred to a separate execution slice.

`ScheduledTask::revision` exposes the exact persisted revision represented by a projection. `ScheduleStore::release(id, expected_revision, reason)` appends exact non-blank caller-owned recovery evidence only when that exact revision remains claimed and returns that immutable intent to pending eligibility. Released schedules survive reopen, preserve their latest exact release reason in load and inventory projections, reappear in due discovery against a sufficient caller-owned cutoff, and may be claimed again. Missing, pending, and cancelled schedules return typed errors without an append; racing releases commit exactly one event and the loser receives typed `ConcurrentModification` evidence identifying the expected and persisted revisions. An earlier claimant acting after an intervening release/reclaim cycle receives the same conflict and cannot release the later claim. Replay accepts direct `created -> materialized`, or `created -> (claimed -> released)*` followed by an optional final claim, pending cancellation, or claimed materialization, and rejects every other ordering fail-closed.

`ScheduleStore::materialize(id, expected_revision, task_id)` atomically appends one `schedule.materialized` event and one existing `task.started` event only when that exact revision remains claimed and the caller-owned task ID has no stream. The task receives the schedule's exact immutable goal; the terminal materialized schedule preserves the exact task ID across load and inventory discovery. A task-ID collision leaves the schedule claimed, and a stale schedule revision leaves no task orphan, including when an earlier claimant acts after an intervening release/reclaim cycle. This explicit-ID method permits materialization only after a claim, including after any number of explicit claim-release recovery cycles.

`ScheduleStore::history(id)` returns every validated creation, claim, release, cancellation, and materialization transition for one exact schedule as a revision-bearing typed `ScheduleHistoryEntry` in stream-revision order. Creation preserves the exact goal and due instant; reason-bearing and task-binding transitions preserve their exact caller-owned values. A missing schedule returns no history. The query decodes and projects the complete lifecycle before exposing any result, so malformed payloads, unsupported events, version gaps, or impossible ordering return an error and no partial prefix.

`ScheduleStore::find_by_task_id(task_id)` provides the reverse read-only provenance lookup for materialized schedules. It validates discovered schedule histories in one SQLite read snapshot and returns the complete schedule bound to the exact caller-owned task identity, or no schedule when the task is unrelated. Persisted corruption that binds one task identity from multiple schedule streams returns typed `AmbiguousTaskBinding` evidence rather than choosing an arbitrary schedule. The query does not inspect task outcome or mutate either lifecycle.

The store never reads wall-clock time. Every post-creation mutation is guarded by an exact persisted revision, preventing an earlier observation from consuming later lifecycle state; exact-ID operations require the caller to supply that revision, while next-due operations select and validate it internally. A revision is not worker identity, a permission grant, a lease, or proof of liveness. Materialization creates inert active task state but does not infer worker failure or lease expiry, create a session, start or advance a workflow, invoke a provider or tool, grant permission, sleep, execute or dispatch task work, retry execution, or interpret recurrence, cron syntax, or time zones. A caller decides when to query, claim, release, or materialize and retains every execution decision.

## Alternatives considered

### Read `SystemTime::now` inside `list_due`

Rejected because ambient time makes selection less deterministic, hides clock authority inside persistence, and complicates tests. A caller-owned cutoff is explicit and can later come from a real or simulated clock.

### Create tasks automatically when schedules become due

Rejected because due discovery does not grant execution authority and an automatic task ID or clock would hide caller policy. Materialization instead always requires a caller-owned task ID and atomically binds the inert task without executing it. Callers may either use the recoverable explicit claim boundary or atomically select and materialize the next due intent.

### Start with cron expressions and time zones

Rejected because recurrence parsing and civil-time semantics add substantial policy before one-shot identity, replay, and ordering exist. A Unix-millisecond instant is sufficient for the first bounded slice.

### Store schedule rows outside the event log

Rejected because schedules require the same immutable identity and fail-closed replay properties as existing kernel aggregates. A separate mutable table would introduce a second persistence model without a demonstrated need.

## Consequences

- Vela can durably record exact one-shot task intent and query due work deterministically.
- Callers retain all clock and execution authority.
- Due results are stable across insertion order and database reopen.
- Full schedule inventory is stable across insertion order and database reopen without requiring callers to know exact IDs.
- A pending schedule can be withdrawn exactly once with durable caller-owned reason evidence; cancelled schedules remain inspectable but are excluded from due work.
- A durable claim prevents duplicate selection; explicit caller-owned release provides inspectable recovery without implying a lease, worker-health detector, or automatic expiry.
- Deterministic next-due claiming removes caller selection boilerplate while retaining caller-owned cutoffs and optimistic persisted concurrency.
- Explicit materialization binds one claimed schedule to one caller-identified active task atomically, without granting execution authority or risking a one-sided persistence result.
- Atomic next-due materialization removes the optional recoverable claim gap when a caller already owns the task identity, while retaining deterministic selection and all-or-nothing schedule/task persistence.
- Exact typed lifecycle history makes every persisted scheduling and recovery transition inspectable without granting lifecycle authority.
- Exact task-to-schedule reverse lookup makes materialization provenance inspectable and rejects ambiguous corrupted bindings.
- Dispatch, worker identity, recurrence, automatic recovery, retries, and execution outcomes require later explicit decisions.
