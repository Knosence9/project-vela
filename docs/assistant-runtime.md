# Single-turn assistant runtime

The `vela-kernel` crate provides a synchronous, provider-neutral boundary that executes one tool-free assistant turn over an existing persisted session.

## Observable contract

- `AssistantProvider` receives the complete ordered persisted transcript after the current human turn has committed. It returns one validated `SessionTurnContent`; provider adapters own request conversion and response validation beyond the kernel's existing non-empty content rule.
- `AssistantRuntime<P>` owns a `SessionStore` and a caller-supplied provider. `AssistantRuntime::open` opens the same typed SQLite event log used by session lifecycle operations and performs no provider work.
- `AssistantRuntime::execute_turn` first appends the supplied content as a human turn, calls the provider exactly once with the resulting transcript, then appends the returned content as an assistant turn. Success returns the updated `Session`; both turns survive reopening the store.
- Missing and closed sessions return `RuntimeError::Session` before provider invocation and append no turn.
- Provider failure returns `RuntimeError::Provider`. The human turn remains durable and no assistant turn is synthesized. `ProviderError` retains the adapter-specific standard error as its source, and `RuntimeError` retains `ProviderError` in its source chain.
- The runtime never retries provider calls because an invocation may have external effects. A caller-requested retry is a new turn invocation and therefore persists another human turn.
- If the assistant append fails after provider success, the session error is returned. The previously committed human turn remains durable; there is no rollback across the external provider call.
- `AssistantProvider` and `AssistantRuntime` are synchronous. The caller owns provider construction, model selection, credentials, timeout behavior, and any serialization needed to ensure only one invocation is in flight for a session.
- Existing session event payloads and replay projections are unchanged. The provider sees existing `SessionTurn` values, so the durable transcript remains the source of truth.

`RuntimeError` is non-exhaustive. Session and provider failures are exposed through `std::error::Error::source`.

## Non-goals

This slice does not add asynchronous execution, streaming, cooperative cancellation, concurrent invocation coordination, automatic retries, system or developer prompts, provider/model metadata, credentials, tools, permissions, token accounting, task lifecycle changes, task observations, cross-aggregate transactions, or storage migration. Task/evidence integration requires a separate contract for partial commits across the session and task aggregates.

See [`session-lifecycle.md`](session-lifecycle.md) for transcript persistence and [`event-log.md`](event-log.md) for durability guarantees.
