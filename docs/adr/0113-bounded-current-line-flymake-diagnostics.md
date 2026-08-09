# ADR-0113: Bounded current-line Flymake diagnostics

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1037](https://github.com/Knosence9/project-vela/issues/1037)
- **Related:** [ADR-0108](0108-main-thread-authoritative-emacs-agent-interface.md), [ADR-0109](0109-observational-emacs-buffer-restriction-context.md)

## Context

ADR-0108 catalogs native Flymake as an intended semantic facility, but protocol version 5 exposes only its availability. External work can observe buffer position and revision while remaining unable to see diagnostics that Emacs has already published for that location.

A complete-buffer diagnostic inventory could be large and disclose unrelated findings. Starting or refreshing backends would add process, timing, and package-specific authority to a synchronous observation. The smallest useful slice is therefore the already-published diagnostics that intersect the current accessible line.

## Decision

Advance the Emacs agent interface to protocol version 6. Expose `diagnostics` as a fourth optional `context.snapshot` section and identify it in capability metadata.

The section is an ordered vector. Vela asks native `flymake-diagnostics` only for the current accessible line, without widening the buffer. Each result contains:

- `start`: the Emacs-native 1-based diagnostic start;
- `end`: the exclusive diagnostic end;
- `type`: the bounded diagnostic type name without a keyword prefix; and
- `text`: the bounded native diagnostic text.

Results are sorted deterministically by start, end, type, and text. At most 128 diagnostics are accepted. Every diagnostic must belong to the current buffer, intersect the requested accessible line, remain within the current restriction, and provide valid non-empty integer bounds, a symbol type, and bounded string text. Zero-width diagnostics are rejected because Emacs 30.1 removes their overlays before `flymake-diagnostics` can publish them; the protocol does not promise mocked state that the native API cannot expose. Marker bounds must also belong to the current buffer. The serialized diagnostic items share a 131,072-character aggregate budget in addition to the 8,192-character per-string bound. Wider, improper, cross-buffer, inaccessible, aggregate-oversized, or malformed native results fail closed. The complete source buffer remains subject to the existing 1,048,576-character synchronous snapshot cap, and the encoder node bound admits a complete four-section response containing 128 diagnostics.

The dispatcher reads only Flymake state already published in the current buffer. It does not enable Flymake, start or refresh a backend, wait for results, expose backend objects, propose fixes, read files, or run processes. Extraction remains on the editor-owner thread and preserves point, mark, narrowing, text and properties, modification state and tick, undo state, and match data.

The explicit `include` width advances from three to four unique sections: `buffer`, `org`, `project`, and `diagnostics`.

## Alternatives considered

### Return every diagnostic in the buffer

Rejected because it broadens disclosure and synchronous traversal beyond the user's current location without a pagination contract.

### Start Flymake and wait for fresh results

Rejected because backend execution and waiting are not bounded read-only observation and could freeze Emacs's owner thread.

### Return backend objects or fix actions

Rejected because backend values are package-specific and non-JSON, while fixes introduce mutation and approval concerns that require separate contracts.

### Return only severity counts

Rejected because counts omit the exact location and message needed to understand the current line while still requiring diagnostic traversal.

## Consequences

- Agents and the human-readable interface can inspect native current-line findings without UI scraping.
- Empty or not-yet-published Flymake state is represented by an empty vector, not by an implicit backend refresh.
- Diagnostic text can contain sensitive tool output; callers must request the section explicitly and future transports must enforce Vela authorization.
- Results are observational and may become stale immediately. Consumers must pair them with buffer identity and text revision when correlating delayed work.
- Diagnostics elsewhere in the buffer, fixes, backend lifecycle, transport, jobs, compilation control, and mutation remain absent.

## Verification

- ERT covers protocol and capability metadata, current-line range dispatch, deterministic shape and ordering, empty state, bounds and metadata failures, aggregate diagnostic output, the complete four-section node boundary, request width, and preservation of live editor state.
- Byte compilation treats warnings as errors.
- The repository-wide `just verify` gate remains authoritative.

## Revisit when

Revisit before adding whole-buffer diagnostic pagination, freshness metadata, backend lifecycle control, fixes, asynchronous transport, or any mutation operation.
