# ADR-0111: Process-local live Emacs buffer identity

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1033](https://github.com/Knosence9/project-vela/issues/1033)
- **Related:** [ADR-0108](0108-main-thread-authoritative-emacs-agent-interface.md), [ADR-0110](0110-observational-emacs-buffer-text-revision.md)

## Context

ADR-0108 requires future delayed callbacks to validate live buffer identity, and ADR-0110 adds character-revision evidence. Protocol version 3 still identifies a buffer only by its name and optional visited file. A buffer may be killed while external work is in flight and a different buffer may later reuse the same name or file. Modification ticks are not globally unique, so those fields cannot fail closed against this ABA case.

Adding targeting or mutation now would prematurely combine transport authentication, lookup, queueing, approval, operation scope, and edit policy. The smallest useful slice is read-only evidence that distinguishes the exact live buffer object observed by Emacs.

## Decision

Advance the Emacs agent interface to protocol version 4. Append `identity` immediately after `file` in the ordered `buffer` context. Its value is an opaque process-local string assigned to the exact live Emacs buffer object on its first Vela snapshot.

Identity allocation runs only through the existing owner-thread request boundary. A monotonically increasing counter prevents token reuse within one Emacs process, including after a buffer is killed and another buffer receives the same name. Process state is retained on the package symbol so unloading and reloading the feature cannot reset the counter. An `eq` hash table with weak keys associates tokens without adding buffer-local state or retaining killed buffers.

The token is equality evidence only. It is stable across repeated snapshots, character changes, renaming, and visited-file changes while the same buffer object remains live. It is not durable across Emacs restarts, does not disclose text, and grants no lookup, read, targeting, or mutation authority. Future delayed mutation must resolve the token to the exact still-live buffer on the owner thread and also validate expected text revision, live restriction and operation scope, and Vela approval evidence.

No operation, context section, include width, transport, text access, filesystem discovery, or mutation capability is added.

## Alternatives considered

### Use buffer name and visited file

Rejected because both can be reused after the observed buffer is killed.

### Use the character modification tick as identity

Rejected because ticks are opaque per-buffer revision evidence, are not globally unique, and may coincide across distinct buffers.

### Expose `sxhash-eq` of the buffer object

Rejected because hashes may collide and object identities may be recycled; neither property provides a deliberate non-reuse contract.

### Store a UUID in a buffer-local variable

Rejected because it mutates source-buffer-local state and requires randomness when a monotonic process-local allocator is sufficient.

## Consequences

- External work can carry ABA-resistant evidence for the exact live buffer it observed.
- The weak association does not keep killed buffers alive, while monotonic allocation prevents their tokens from being issued again during the process lifetime.
- Consumers must treat identity as opaque and pair it with protocol version and the other required preconditions.
- Restarting Emacs invalidates every previously observed identity.
- Mutation and transport authority remain absent.

## Verification

ERT pins protocol version and deterministic field order, verifies stable repeated and post-edit identity, distinguishes simultaneous buffers, rejects same-name kill-and-recreate and feature-reload reuse through different tokens, and verifies the first snapshot adds no buffer-local state. Existing editor-state preservation, owner-thread, bounded encoding, and complete repository quality gates remain green.
