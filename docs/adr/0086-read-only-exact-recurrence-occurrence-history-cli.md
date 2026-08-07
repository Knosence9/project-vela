# ADR-0086: Read-only exact recurrence occurrence history CLI

- **Status:** accepted
- **Date:** 2026-08-07
- **Decision and execution issue:** [#971](https://github.com/Knosence9/project-vela/issues/971)
- **Related:** ADR-0046, ADR-0071, ADR-0073, ADR-0085

## Context

ADR-0085 provides complete typed lifecycle history for one exact persisted recurrence occurrence. Without a CLI adapter, an operator must write Rust or decode raw event-log rows, duplicating payload-version, canonical-provenance, and lifecycle validation.

The adapter must preserve the kernel's exact-coordinate and read-only authority boundary. It must not turn historical inspection into cross-occurrence discovery or recurrence lifecycle authority.

## Decision

Add `vela-dev recurrence occurrence-history DATABASE RECURRENCE_ID OFFSET`. Clap parses the caller-owned zero-based offset as `u64`; the command validates the exact recurrence ID before storage access and opens only the selected existing database through `RecurrenceStore::open_read_only`.

The adapter delegates complete selected-coordinate replay to `RecurrenceStore::occurrence_history`. Success emits one compact deterministic JSON object with exact `recurrence_id`, `offset`, and revision-ordered `history`. A missing occurrence stream emits `history: null`. Present entries use these tagged forms:

- `persisted`: exact goal, Unix-millisecond instant, and immutable definition revision
- `claimed`
- `released`: exact caller-authored recovery reason
- `materialized`: exact caller-owned task ID

The exact recurrence ID and offset remain top-level coordinates rather than being repeated in the persistence entry. All caller-authored strings are JSON escaped.

Invalid IDs emit `invalid_recurrence_id` before storage access. Missing storage, schema, selected definition or occurrence replay, projection, and serialization failures emit one escaped `recurrence_occurrence_history_failed` diagnostic, return non-zero, and emit no partial stdout.

The command reads no ambient clock, mutates no state, scans no unrelated occurrence coordinate, and grants no persistence, cancellation, claim, release, materialization, discovery, worker, lease, dispatch, permission, retry, or execution authority.

## Alternatives considered

### Expose raw event-log rows

Rejected because callers could consume malformed, divergent, unsupported, or impossible histories and would duplicate kernel validation.

### Add history to current occurrence lookup

Rejected because current-state projection and complete historical evidence have different absence and output contracts. Keeping an explicit command avoids changing stable lookup JSON.

### Add bounded cross-occurrence history discovery

Rejected because the requested operational boundary identifies one exact coordinate. Discovery requires a separate bounded contract and corruption-isolation policy.

## Consequences

- Operators can audit persistence, recovery, claims, and materialization without writable storage or raw payload decoding.
- Missing coordinates remain distinct from selected corruption.
- Corruption in unrelated coordinates cannot block exact inspection.
- No durable schema, event, lifecycle transition, clock, or execution authority is added.

## Verification

RED→GREEN CLI tests prove repeated claim/release and materialization history with exact revisions and escaped values, missing-coordinate output, read-only reopen, pre-storage ID validation, missing-storage non-creation, selected corruption rejection, and unrelated-coordinate isolation. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding cross-occurrence history discovery, destructive deletion, claim interruption, undo or resume semantics, clocks, workers, leases, dispatch, permissions, retries, or execution.
