# Tool invocation and permission boundary

The `vela-kernel` crate defines a synchronous, provider-neutral protocol for authorizing one in-process tool adapter invocation. This boundary establishes ownership and ordering only; Vela does not yet ship a real tool or connect tools to `AssistantRuntime`.

## Ownership

- The kernel owns `ToolId`, `ToolEffect`, `ToolRequest`, `PermissionDecision`, `ToolAuthorizer`, `Tool`, `ToolError`, `ToolInvocationError`, and `invoke_tool`.
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

## Durability and compatibility

This protocol is deliberately non-durable. It opens no store and writes no event, session turn, task observation, permission grant, invocation intent, result, or error. A later approved runtime slice may introduce dedicated durable invocation evidence, but existing evidence kinds are not repurposed by implication and model output is not deterministic verification.

Existing event payloads and replay, session/task stores, `AssistantProvider`, and `AssistantRuntime` remain unchanged.

## Non-goals

This slice does not add provider tool-call parsing, runtime orchestration, automatic or model-owned permission, permission persistence, reusable grants, registry/discovery, JSON Schema publication, real filesystem/network/process/credential tools, retries, timeout implementation, asynchronous execution, cooperative cancellation, sandboxing, isolation, deterministic verification ingestion, event migration, or identity/personality policy.
