# ADR-0117: Bounded outbound Emacs JSON framing

- **Status:** accepted
- **Date:** 2026-08-09
- **Decision and execution issue:** [#1047](https://github.com/Knosence9/project-vela/issues/1047)
- **Related:** [ADR-0115](0115-bounded-in-process-emacs-json-adapter.md), [ADR-0116](0116-bounded-newline-delimited-emacs-json-framing.md)

## Context

ADR-0115 returns deterministic bounded JSON characters and ADR-0116 establishes bounded inbound newline framing. A later local transport still needs one unambiguous way to turn a response string into wire bytes. Letting each transport choose its own character encoding, delimiter escaping, or byte limit would make outbound behavior differ from the inbound canonical UTF-8 contract.

Opening a process or socket would additionally select a peer, authentication, lifecycle, queueing, callback, and error-delivery policy. None of those authorities is required to encode one already-produced response safely.

## Decision

Keep protocol version 7 because no semantic operation or response shape changes. Add public pure helper `vela-agent-json-frame-encode(payload)`. The caller must supply the deterministic JSON string returned by `vela-agent-handle-json`; the helper deliberately does not parse JSON again.

The payload may be an Emacs multibyte string or an ASCII-only unibyte string, because the deterministic encoder can return the latter when a response contains only ASCII. Unibyte raw characters above ASCII, Emacs raw eight-bit characters, surrogate code points, scalars above U+10FFFF, and strings that do not survive an exact canonical UTF-8 encode/decode round trip fail as `vela-agent-protocol-error`.

Literal CR and LF are rejected so one call cannot inject another record. The canonical UTF-8 payload is capped at 262,144 bytes independently of the existing 262,144-character JSON encoder bound. The returned value is an unibyte string containing those bytes followed by exactly one LF; the delimiter is not counted against the payload bound.

The helper does not parse or dispatch JSON, open or control a process or socket, select or authenticate a peer, keep queue or remainder state, schedule a timer or callback, define transport error delivery, or grant editor or mutation authority.

## Alternatives considered

### Require every input string to be multibyte

Rejected because deterministic ASCII JSON can legitimately be represented by an Emacs unibyte string. Rejecting all unibyte strings would reject the actual output being framed. The authority boundary is canonical Unicode content, so ASCII-only unibyte input is safe while raw high bytes fail closed.

### Parse JSON again before framing

Rejected because `vela-agent-handle-json` and the deterministic response encoder are already the syntax and shape authorities. Re-parsing would duplicate validation without improving framing safety.

### Escape literal newlines in the helper

Rejected because changing payload characters would make this layer another JSON encoder. Deterministic JSON already escapes string content; a literal delimiter indicates misuse and fails closed.

### Write directly to a process

Rejected because process ownership, peer selection, startup and shutdown, backpressure, queueing, callbacks, and error delivery require a separate explicit transport contract.

## Consequences

- Later local transports can reuse symmetric bounded canonical UTF-8 framing in both directions.
- Character-bounded response JSON cannot expand past the independent outbound byte budget unnoticed.
- Each successful call produces exactly one LF-terminated unibyte frame.
- Callers remain responsible for supplying trusted deterministic JSON and for all transport lifecycle policy.
- There is still no external transport, queue, or asynchronous callback.

## Verification

RED→GREEN ERT coverage pins ASCII and Unicode output, exact delimiter bytes, raw-byte and delimiter rejection, invalid Unicode rejection, and exact and oversized ASCII and multibyte byte boundaries. Batch byte compilation rejects warnings, and the complete repository quality gate must remain green.

## Revisit when

Revisit before a transport opens or controls processes or sockets, selects peers or authentication, stores queue state, schedules editor callbacks, defines error delivery or restart policy, adds JSON-RPC envelopes, or grants any mutation authority.
