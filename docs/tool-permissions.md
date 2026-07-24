# Tool invocation and permission boundary

The `vela-kernel` crate defines a synchronous, provider-neutral protocol for authorizing one in-process tool adapter invocation. Callers may use it without persistence through `invoke_tool`, register and resolve adapters through `ToolRegistry`, wrap an invocation with metadata-only durable evidence through `invoke_tool_durable`, immutably attribute that evidence to an active task through `invoke_tool_for_task_durable`, or compose registry lookup with that task-associated path through `ToolRegistry::invoke_for_task_durable`. Vela does not yet ship a real tool or connect tools to `AssistantRuntime`.

## Ownership

- The kernel owns `ToolId`, `ToolEffect`, `ToolRequest`, `PermissionDecision`, `ToolAuthorizer`, `Tool`, `ToolRegistry`, registry/invocation errors, and the invocation functions.
- Extensions own concrete `Tool` adapters, input/output schema validation, honest effect declarations, timeout behavior, idempotency, and any external resources they access.
- Callers own `ToolAuthorizer` policy. A model cannot authorize itself, and the kernel supplies no implicit allow policy.

`ToolId` is an opaque non-blank stable identifier. Tool inputs and outputs are JSON-shaped values so the protocol does not couple the kernel to one provider or extension representation. Each adapter remains responsible for validating the structured input it accepts.

## Effects and permission scope

A tool declares its maximum effect before authorization:

- `Pure` neither observes nor mutates state outside the current process.
- `ExternalRead` observes external state without intending mutation.
- `ExternalWrite` mutates external state without intending destructive removal.
- `Destructive` may delete, irreversibly replace, or otherwise destructively mutate external state.

The vocabulary is non-exhaustive so later boundaries can add distinctions without treating unknown effects as safe. The declaration is permission metadata, not proof: in-process adapters are currently trusted to return stable metadata without external effects and to classify invocation effects honestly. Sandboxing and independent effect enforcement are future work.

Every invocation, including `Pure`, is presented to the caller-owned authorizer. `ToolRequest` exposes the exact tool ID, declared effect, and input that would be passed to the adapter. An allow or deny decision applies only to that invocation. There are no reusable grants, global approvals, or default authorization.

## Ordering and failures

`invoke_tool` performs this sequence:

1. read the adapter ID and declared effect;
2. ask the authorizer once about the exact request;
3. on denial, return `ToolInvocationError::Denied` and do not call the adapter;
4. on allowance, call the adapter once and return its exact output or a source-preserving error.

The kernel does not retry. A caller-requested retry is a new invocation with a new permission decision. Execution is synchronous; callers and adapters own timeout behavior until a separately approved asynchronous cancellation boundary exists.

Authorization itself occurs before the adapter can produce a tool effect. After an allowed adapter starts, it may partially affect external state before returning `ToolError`. The kernel reports the error without retry or rollback; the adapter owns idempotency and any compensation protocol.

## In-memory registry

`ToolRegistry` is a process-local owner and deterministic directory for extension-provided adapters. Registration uses each adapter's existing stable `ToolId`; duplicate IDs return `ToolRegistryError::DuplicateId` without replacing or invoking the original or duplicate adapter. There is deliberately no removal, replacement, alias, reload, or persistence policy.

`ToolRegistry::metadata` returns cloned `ToolMetadata` entries in ascending ID order, independent of registration order. Metadata contains only the adapter ID and declared maximum effect. Enumeration does not authorize or invoke an adapter.

`ToolRegistry::invoke` resolves a caller-supplied ID and delegates to the existing `invoke_tool` protocol. An unknown ID returns `ToolRegistryInvocationError::NotFound` before authorization. Permission denial and sourced adapter failures remain available through `ToolRegistryInvocationError::Invocation`; registry dispatch adds no retry, grant, or alternative authorization path.

`ToolRegistry::invoke_for_task_durable` resolves a caller-supplied ID before delegating to the existing `invoke_tool_for_task_durable` protocol. An unknown ID returns `DurableToolRegistryInvocationError::NotFound` before task validation, durable intent, authorization, or adapter execution. Known adapters retain the existing task preconditions, duplicate-invocation protection, immutable attribution, permission ordering, metadata-only retention, terminal-persistence behavior, and exact in-memory result. Durable failures are wrapped without replacing their source chain. Registry dispatch adds no second persistence or execution protocol.

## Durable invocation evidence

`invoke_tool` remains non-durable and API-compatible. `invoke_tool_durable` is an additive wrapper using `ToolInvocationStore` and a caller-supplied, non-blank `ToolInvocationId`. Each ID owns an independent `tool-invocation:<id>` event stream. `invoke_tool_for_task_durable` adds an immutable task association while leaving the unassociated wrapper unchanged.

The wrapper writes this ordered prefix:

1. `tool.invocation_intended`, containing `ToolId`, declared `ToolEffect`, and an optional immutable `TaskId`, before authorization;
2. exactly one of `tool.invocation_denied`, `tool.invocation_succeeded`, or `tool.invocation_failed` after the in-memory result is known.

There is no durable “allowed” event. Intent is committed first, then the existing invocation protocol authorizes once and either skips or calls the adapter once. A duplicate invocation ID fails before authorization or execution.

For a task-associated invocation, the target task must exist and be active. The store validates its current stream version and atomically appends intent only while that exact version remains current. Missing and terminal tasks fail with typed pre-execution errors. Any concurrent task mutation—including an observation or terminal transition—returns `ToolInvocationStoreError::TaskChanged`; Vela does not retry validation, authorization, or adapter execution. Once intent commits, later task transitions do not alter the association or invocation lifecycle.

Unassociated version-1 intent payloads replay with no task ID. Associated intents use additive payload version 2 and require one valid task ID. `ToolInvocation::task_id` projects either shape through both `load` and `list`. Association is stored only on the invocation stream: there is no task-side mutable index or task-filtered query. Callers can filter the deterministic global list when needed. Task-level attribution deliberately precedes attempt-level correlation because a provider tool call can occur before an assistant attempt observation exists.

`ToolInvocationStore::load` replays a valid stream as `Pending`, `Denied`, `Succeeded`, or `Failed`. Valid history is exactly one intent followed by at most one terminal event. Terminal-first, repeated-intent, repeated-terminal, and post-terminal histories fail closed. An absent stream loads as absent.

`ToolInvocationStore::list` discovers invocation streams from their existing intent events, replays every invocation from one consistent SQLite read snapshot, and returns them in ascending `ToolInvocationId` order. Pending and all terminal statuses are included. Empty stores return an empty list, unrelated event streams are excluded, reopening does not change the result, and listing appends no events or mutable index. A malformed owning stream ID returns `ToolInvocationStoreError::InvalidStreamId`; malformed payloads and histories retain their replay/store errors.

### Retention boundary

The event payload deliberately excludes exact input, exact output, adapter error text, permission-policy details, credentials, and other potentially sensitive or high-volume values. It retains only invocation/task/tool identities, declared effect, and terminal status. Terminal payloads are empty. Exact output and source-preserving invocation errors remain available only to the immediate caller.

This is a storage contract, not a configurable redaction implementation: richer payload capture requires a separate approved size, schema-version, and redaction boundary. Operators must still protect tool IDs, effect declarations, stream IDs, and the SQLite database as operational metadata.

### Crash and append semantics

An intent-only `Pending` stream means authorization and/or adapter execution may have been attempted. It is intentionally ambiguous and must never trigger automatic resume or retry. In particular, an external effect may have completed before a crash prevented the terminal append.

If the terminal append fails after authorization or execution, `DurableToolInvocationError::TerminalPersistence` returns both the persistence error and the exact in-memory invocation result. The wrapper does not retry, roll back, or hide a successful output or sourced adapter failure. Callers must treat the durable stream as pending and resolve ambiguity outside this protocol.

Existing event families, session/task replay, `AssistantProvider`, and `AssistantRuntime` remain unchanged.

## Non-goals

This slice does not add provider tool-call parsing, runtime orchestration, automatic or model-owned permission, persisted allow grants, reusable grants, registry persistence/removal/replacement/reload, JSON Schema publication, real filesystem/network/process/credential tools, session or attempt association, task-filtered discovery, a task-side invocation index, retries, timeout implementation, asynchronous execution, cooperative cancellation, sandboxing, isolation, rich invocation payload retention, deterministic verification ingestion, event migration, or identity/personality policy.
