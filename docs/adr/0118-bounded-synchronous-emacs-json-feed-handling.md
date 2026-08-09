# ADR-0118: Bounded synchronous Emacs JSON feed handling

- **Status:** accepted
- **Date:** 2026-08-09
- **Decision and execution issue:** [#1051](https://github.com/Knosence9/project-vela/issues/1051)
- **Related:** [ADR-0115](0115-bounded-in-process-emacs-json-adapter.md), [ADR-0116](0116-bounded-newline-delimited-emacs-json-framing.md), [ADR-0117](0117-bounded-outbound-emacs-json-framing.md)

## Context

ADR-0115 through ADR-0117 establish bounded JSON dispatch and symmetric canonical UTF-8 newline framing. A later local transport would still have to compose those boundaries consistently: preserve arrival order, retain partial input exactly, dispatch only complete frames on the editor-owner thread, frame every response, and fail without returning a plausible partial batch.

Opening a process or socket would additionally select a peer, authentication, lifecycle, backpressure, callbacks, and error delivery. Those authority and security choices are not necessary to define safe synchronous composition of the accepted helpers.

## Decision

Keep protocol version 7 because semantic operations and response shapes do not change. Add public synchronous helper `vela-agent-handle-json-feed(pending, chunk)`. It first requires the editor-owner thread, then passes caller-owned unibyte input to `vela-agent-json-frame-feed`. Each complete decoded request is handled in arrival order by `vela-agent-handle-json`, and each deterministic response is converted to canonical unibyte wire bytes by `vela-agent-json-frame-encode`.

Success returns an ordered object with a `responses` vector of complete LF-terminated unibyte frames and the exact unconsumed unibyte `remainder`. The caller owns and must supply that remainder to a later call. The existing framer limit of 16 complete requests per feed also bounds the returned vector and, together with the 262,144-byte response payload limit, bounds aggregate returned response bytes.

Any framing, decoding, request validation, dispatch, encoding, or response framing error rejects the complete call and returns no result. Errors already classified by the composed public boundaries remain `vela-agent-protocol-error`; this composition does not relabel unexpected native Emacs errors. Previously evaluated operations are read-only, so rejection cannot expose a partial response batch or partially apply editor mutations.

The helper does not open or control a process or socket, select or authenticate a peer, retain remainder or queue state, schedule a timer or callback, provide backpressure, retry requests, define transport error envelopes, start external work, or grant editor mutation authority.

## Alternatives considered

### Let every transport compose the three helpers

Rejected because transport implementations could disagree about request ordering, owner-thread preflight, partial remainders, response framing, or partial-batch errors despite sharing the same codecs.

### Return successful responses before a later request fails

Rejected because a partial batch is ambiguous to callers and can encourage retrying already-observed reads. The bounded feed is small enough to accumulate responses locally and return only after every complete request succeeds.

### Open a local socket or child process now

Rejected because peer identity, authentication, lifecycle, backpressure, error delivery, and restart policy are separate authority decisions. Synchronous composition is useful without prematurely choosing them.

## Consequences

- A later transport can delegate one complete bounded synchronous feed without duplicating parsing, dispatch, encoding, ordering, or remainder semantics.
- At most 16 read-only requests are evaluated per call, and every response remains independently byte bounded.
- A feed error yields no partial result, although earlier read-only snapshots in that attempted feed may already have been observed internally.
- There is still no external transport, queue, callback, authentication, asynchronous job, cancellation, or mutation authority.

## Verification

RED→GREEN ERT coverage pins split input, ordered multiple responses, exact unibyte response framing and remainder preservation, complete-feed failure, and worker-thread rejection. Batch byte compilation rejects warnings, and the complete repository quality gate must remain green.

## Revisit when

Revisit before opening or controlling a process or socket, selecting or authenticating a peer, retaining transport state, scheduling callbacks, defining backpressure or error delivery, adding asynchronous jobs or cancellation, or granting any mutation authority.
