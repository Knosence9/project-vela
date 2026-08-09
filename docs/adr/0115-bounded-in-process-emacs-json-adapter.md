# ADR-0115: Bounded in-process Emacs JSON adapter

- **Status:** accepted
- **Date:** 2026-08-09
- **Decision and execution issue:** [#1043](https://github.com/Knosence9/project-vela/issues/1043)
- **Related:** [ADR-0108](0108-main-thread-authoritative-emacs-agent-interface.md), [ADR-0114](0114-bounded-native-emacs-compilation-context.md)

## Context

ADR-0108 defines JSON-compatible request and response shapes for a future local transport, but callers can currently enter the protocol only through Emacs Lisp values. The deterministic response encoder already establishes a bounded outgoing wire representation. Letting each later process filter or local client choose its own decoder would duplicate validation, risk inconsistent JSON null, false, array, object, and duplicate-key semantics, and blur the editor-owner-thread boundary.

A socket, subprocess, JSON-RPC envelope, framing strategy, queue, or timer would introduce separate lifecycle, authentication, backpressure, cancellation, and concurrency decisions. Those authorities are not required to establish one reusable wire codec.

## Decision

Keep protocol version 7 because no operation or response shape changes. Add public `vela-agent-handle-json`, an in-process adapter that accepts exactly one JSON request string, dispatches it through `vela-agent-handle-request`, and returns the deterministic output of `vela-agent-encode-response`.

The adapter rejects input longer than 262,144 Emacs characters before parsing. Native `json-parse-string` first validates strict JSON syntax. The adapter then parses in a temporary buffer with native `json-read`, string object keys, vectors for arrays, `:null` for JSON null, and `:false` for JSON false. Parsing must consume the complete input except trailing JSON whitespace. Malformed syntax, trailing values or text, and non-object roots fail as `vela-agent-protocol-error`; parser-specific errors do not escape.

Objects remain ordered alists so duplicate members survive decoding. A recursive decoded-value validator rejects duplicate keys at every object depth and caps nesting at 16 levels, each object or array at 128 members, and the complete request at 1,024 value nodes. The existing eight-field, key-length, operation, section, buffer, response traversal, output-size, and editor-owner-thread checks remain authoritative. The adapter checks the owner thread before parsing, so worker threads cannot use decoding as an alternate editor entry point.

This function is only a synchronous in-process wire adapter. It does not listen, read a process, frame multiple messages, enqueue work, schedule callbacks, or grant any operation beyond the existing read-only dispatcher.

## Alternatives considered

### Add JSON-RPC and a local socket now

Rejected because method envelopes, IDs, notifications, framing, peer authentication, queue bounds, lifecycle, and cancellation need their own contract. A codec can be tested and reused without prematurely selecting those authorities.

### Use `json-parse-string` with hash-table objects

Rejected because hash tables discard duplicate member evidence before the protocol validator can reject ambiguous requests. Ordered alists preserve exact member order and duplicates.

### Accept the first JSON value and ignore trailing input

Rejected because silently ignoring a second value or trailing text makes framing ambiguous and lets callers believe more input was handled than the protocol actually consumed.

### Expose parser errors directly

Rejected because parser-specific conditions are an implementation detail and would make the public failure taxonomy depend on Emacs JSON internals.

## Consequences

- Local callers can exercise the exact bounded wire representation without constructing Emacs Lisp request values.
- A future transport has one decode/dispatch/encode function to invoke on the editor-owner thread.
- Duplicate keys at any object depth, non-object roots, malformed input, trailing input, deeply nested or wide decoded values, and oversized input fail closed.
- Character count is the current conservative input budget; any future byte-framed transport must establish a separate byte bound before decoding.
- Protocol version remains 7 because semantic operations and response shapes are unchanged.

## Verification

RED→GREEN ERT coverage pins exact capabilities and context round trips, JSON null/false/array markers, malformed syntax (including trailing commas) and trailing input rejection, non-object rejection, top-level and nested duplicate-key rejection, pre-parse input bounding, and worker-thread rejection. Batch byte compilation rejects warnings, and the complete repository quality gate must remain green.

## Revisit when

Revisit before adding byte framing, JSON-RPC envelopes, sockets, subprocess filters, multiple requests, streaming, queues, timers, asynchronous jobs, cancellation, authentication, approvals, mutation, or any transport-specific lifecycle.
