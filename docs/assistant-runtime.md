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

`AssistantRuntime::execute_task_correction_turn` is the explicit correction-producing counterpart. The caller additionally supplies the ID of an earlier attempt in the same task. Before writing a transcript turn or invoking the provider, the runtime applies the same active/association checks and asks the task aggregate to reject a duplicate correction ID, missing parent, or non-attempt parent. It deliberately does not generalize the API to a caller-selected evidence kind: provider turns produce attempts through `execute_task_turn` or corrections through this operation; diagnostics and verifications remain store-level evidence.

After preflight, correction execution uses the same durable human turn, one provider call, and durable assistant turn sequence. It then appends a `TaskObservationKind::Correction` with the caller-owned ID, exact assistant content, and supplied parent-attempt ID. Preflight is not a lock: if the task changes after preflight, the authoritative append reports the winning terminal state or observation error after both transcript turns have committed.

`AssistantRuntime::complete_task_turn` is the explicit caller-requested completion boundary. The caller supplies the final human content and an attempt observation ID; the runtime owns the ordering, but it does not decide autonomously that the task is complete. It preflights the active associated task, uniqueness of the attempt ID, and session writability before invoking the provider. It then commits the human and assistant turns, validates the assistant text as both attempt evidence and task output, appends an `Attempt`, and completes the task with exactly the same text. The returned `TaskTurnOutcome` contains the durable transcript and completed task projections. This model response is task output, not `Verification`; verification remains independently observed store-level evidence.

Completion preserves its ordered partial commits. Provider or assistant-append failure writes no attempt or completion. Invalid attempt/output text or an authoritative attempt-append failure preserves both transcript turns but writes no completion. If completion loses a race after the attempt commits, the attempt remains durable and `RuntimeError::Task` exposes the authoritative terminal state. Preflight is not a lock, no provider call is retried, and no cross-stream rollback is attempted.

`AssistantRuntime::fail_task_turn` is the symmetric caller-requested failure boundary. The caller supplies validated `TaskFailure` data in addition to the final human content and attempt ID, so invalid or whitespace-only diagnostics cannot enter the operation. The runtime preflights the same task, observation, and session constraints, commits both transcript turns, appends the provider response only as an `Attempt`, and then fails the task with the exact caller-owned diagnostic. Model text is never reclassified as `Diagnostic` or `Verification` evidence.

Failure execution preserves the same ordered durable prefixes. Provider or assistant-append failure writes no attempt or terminal failure. Invalid attempt text and authoritative attempt-append errors preserve both transcript turns without failing the task. If the terminal failure loses a race after the attempt commits, that attempt remains durable and the winning terminal state is returned as `RuntimeError::Task`. No provider call is retried and no cross-stream rollback is attempted.

`AssistantRuntime::cancel_task_turn` is the caller-requested cancellation counterpart. The caller supplies validated `TaskCancellation` data along with the final human content and attempt ID. The runtime preflights the active associated task, attempt uniqueness, and writable session, then commits both transcript turns, the provider response as an `Attempt`, and the exact caller-owned cancellation reason in that order. Model text is not the cancellation reason and is not reclassified as another evidence kind.

Cancellation preserves the same ordered durable prefixes and race behavior as failure. Provider or assistant-append failure writes no attempt or cancellation; invalid attempt text and authoritative observation errors preserve the transcript without cancelling; and a terminal race after the attempt preserves that attempt plus the winning state. This operation records terminal intent after a final turn—it does not interrupt a provider call or provide cooperative in-flight cancellation.

The task-associated operations deliberately do not create a cross-stream transaction around an external provider call:

- Provider or assistant-append failure follows the single-turn contract: the human turn remains durable and no task observation is appended.
- Observation failure after assistant persistence returns `RuntimeError::Task` and preserves both transcript turns. The runtime neither retries nor rolls back the provider call. A duplicate caller-supplied observation ID is one deterministic example for attempt turns; correction turns reject known duplicates during preflight but can still encounter a racing duplicate at the authoritative append.
- Session content permits whitespace-only text while task observation text does not. If a provider returns whitespace-only content, both transcript turns remain durable and `RuntimeError::InvalidAttemptText` or `RuntimeError::InvalidCorrectionText` reports why no evidence was appended.
- Task-store and invalid-observation-text failures are retained in the `std::error::Error::source` chain. `TaskNotAssociated` is a source-free runtime domain error.

`RuntimeError` is non-exhaustive. Wrapped session, provider, task-store, and observation-text failures are exposed through `std::error::Error::source`.

## Non-goals

This slice does not add asynchronous execution, streaming, cooperative cancellation, concurrent invocation coordination, automatic retries, system or developer prompts, provider/model metadata, credentials, tools, permissions, token accounting, automatic task creation, association, completion or failure, generic caller-selected evidence kinds, runtime diagnostic or verification turns, cross-aggregate transactions, or storage migration.

See [`session-lifecycle.md`](session-lifecycle.md) for transcript persistence and [`event-log.md`](event-log.md) for durability guarantees.
