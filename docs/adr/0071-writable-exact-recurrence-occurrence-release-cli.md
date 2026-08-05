# ADR-0071: Writable exact recurrence occurrence release CLI

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#937](https://github.com/Knosence9/project-vela/issues/937)
- **Related:** ADR-0051, ADR-0068, ADR-0069, ADR-0070

## Context

ADR-0069 defines exact-revision recovery for one claimed persisted recurrence
occurrence. ADR-0070 exposes the preceding exact claim through the developer
CLI, but operators still need custom Rust code to record explicit recovery and
restore availability.

The smallest responsible adapter preserves one caller-selected coordinate, one
observed occurrence revision, and exact caller-authored recovery evidence. It
must not infer worker death or introduce lease, dispatch, retry, or execution
policy.

## Decision

Add:

```text
vela-dev recurrence release DATABASE RECURRENCE_ID OFFSET EXPECTED_OCCURRENCE_REVISION REASON
```

Clap parses the offset and occurrence revision as non-negative `u64` values. The
command validates `RECURRENCE_ID` through `RecurrenceId` and `REASON` through
`RecurrenceOccurrenceRelease` before storage access, opens only the selected
database through `RecurrenceStore::open`, and delegates strict replay,
revision-before-lifecycle validation, concurrency, and append to
`RecurrenceStore::release_occurrence`.

Success emits one compact deterministic JSON object containing exact
`recurrence_id`, `goal`, `offset`, `unix_millis`, `definition_revision`,
resulting `occurrence_revision`, and `latest_release`. Serde JSON escaping
preserves caller-authored identity, goal, and release text.

Invalid identity and blank reason emit `invalid_recurrence_id` and
`invalid_recurrence_occurrence_release` before storage access. Missing
provenance, stale revision, available or materialized lifecycle, malformed
durable history, contention, read-only storage, open, replay, append, and
serialization failures emit `recurrence_occurrence_release_failed`, return
non-zero, and emit no stdout. Exact-version append remains authoritative, so
storage and transition rejection append no recovery evidence. As with the
existing writable adapters, response serialization follows a successful durable
append; the fixed string-and-integer projection is infallible with the selected
serializer, but a future serialization failure would report the already-durable
release rather than roll it back.

The command scans no unrelated coordinate, reads no ambient clock, identifies
no worker, infers no liveness failure, expires no lease, and grants no dispatch,
retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Infer recovery from elapsed time

Rejected because the CLI has no worker-liveness oracle and ambient time cannot
prove that selected work was abandoned.

### Release without an exact observed revision

Rejected because unconditional recovery could overwrite a concurrent claim,
release, or materialization transition.

### Combine claim and release

Rejected because reservation and recovery are distinct consequential
transitions with different evidence and failure contracts.

## Consequences

- Scripts can explicitly recover one abandoned exact claim without custom Rust
  code or inferred liveness.
- Output carries the resulting revision needed for exact reclaim or direct
  materialization.
- Validation precedes storage access and rejected storage transitions append
  nothing; successful release remains authoritative over response delivery.
- Callers retain worker, lease, retry, materialization, and execution policy.

## Verification

Strict RED→GREEN CLI integration tests cover deterministic escaped JSON, exact
release evidence, resulting revision, durable reopen, reclaim at that revision,
validation before storage access, missing provenance, stale revision, available
and materialized state, and no release mutation on rejection. The complete
repository quality gate must remain green.

## Revisit when

Reconsider before adding claimed inventory, claim-next selection, generated
task identity, worker identity, leases or expiry, ambient clocks, dispatch,
retries, permissions, or execution.