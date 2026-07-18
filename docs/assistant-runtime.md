# Single-turn assistant runtime

The `vela-kernel` crate provides a synchronous, provider-neutral boundary that executes one tool-free assistant turn over an existing persisted session.

## Observable contract

- `AssistantProvider` receives the complete ordered persisted transcript after the current human turn has committed. It returns one validated `SessionTurnContent`; provider adapters own request conversion and response validation beyond the kernel's existing non-empty content rule.
- `AssistantRuntime<P>` owns session and task stores over the same typed SQLite event log plus a caller-supplied provider. `AssistantRuntime::open` opens those stores and performs no provider work.
- `AssistantRuntime::execute_turn` first appends the supplied content as a human turn, calls the provider exactly once with the resulting transcript, then appends the returned content as an assistant turn. Success returns the updated `Session`; both turns survive reopening the store.
- Missing and closed sessions return `RuntimeError::Session` before provider invocation and append no turn.
- Provider failure returns `RuntimeError::Provider`. The human turn remains durable and no assistant turn is synthesized. `ProviderError` retains the adapter-specific standard error as its source, and `RuntimeError` retains `ProviderError` in its source chain.
- The runtime never retries provider calls because an invocation may have external effects. A caller-requested retry is a new turn invocation and therefore persists another human turn.
- If the assistant append fails after provider success, the session error is returned. The previously committed human turn remains durable; there is no rollback across the external provider call.
- `AssistantProvider` and `AssistantRuntime` are synchronous. The caller owns provider construction, model selection, credentials, timeout behavior, and any serialization needed to ensure only one invocation is in flight for a session.
- Existing session event payloads and replay projections are unchanged. The provider sees existing `SessionTurn` values, so the durable transcript remains the source of truth.

## Task-associated turns

`AssistantRuntime::execute_task_turn` connects the same single-turn sequence to one existing task. The caller supplies the task ID, human content, and a caller-owned observation ID. The task must be active and already associated with a session; missing, completed, cancelled, failed, and unassociated tasks fail before a transcript write or provider invocation. A closed associated session is rejected by the existing session boundary before provider invocation.

On success, the runtime returns a `TaskTurnOutcome` containing the durable `Session` and `Task` projections. After both transcript turns commit, it appends one `TaskObservationKind::Attempt` whose ID is the supplied ID and whose text is exactly the assistant content. Reopening either store reproduces the returned projections.

The operation deliberately does not create a cross-stream transaction around an external provider call:

- Provider or assistant-append failure follows the single-turn contract: the human turn remains durable and no task observation is appended.
- Observation failure after assistant persistence returns `RuntimeError::Task` and preserves both transcript turns. The runtime neither retries nor rolls back the provider call. A duplicate caller-supplied observation ID is one deterministic example.
- Session content permits whitespace-only text while task observation text does not. If a provider returns whitespace-only content, both transcript turns remain durable and `RuntimeError::InvalidAttemptText` reports why no attempt observation was appended.
- Task-store and invalid-attempt-text failures are retained in the `std::error::Error::source` chain. `TaskNotAssociated` is a source-free runtime domain error.

`RuntimeError` is non-exhaustive. Wrapped session, provider, task-store, and attempt-text failures are exposed through `std::error::Error::source`.

## Non-goals

This slice does not add asynchronous execution, streaming, cooperative cancellation, concurrent invocation coordination, automatic retries, system or developer prompts, provider/model metadata, credentials, tools, permissions, token accounting, automatic task creation, association, completion or failure, structured observation episodes, cross-aggregate transactions, or storage migration.

See [`session-lifecycle.md`](session-lifecycle.md) for transcript persistence and [`event-log.md`](event-log.md) for durability guarantees.
