# Emacs agent interface

Vela's first Emacs integration is a read-only, model-neutral interface over native editor and Org state. It is deliberately small: Emacs remains responsive and authoritative for live editor state while expensive agent work stays in external workers.

The concurrency and authority decision is recorded in [ADR-0108](adr/0108-main-thread-authoritative-emacs-agent-interface.md).

## Load and open

From the repository root:

```elisp
(add-to-list 'load-path (expand-file-name "emacs" default-directory))
(require 'vela-agent-mode)
```

Open an Org or source buffer, then run:

```text
M-x vela-agent-interface-open
```

The resulting `*Vela Agent Interface*` buffer uses `vela-agent-interface-mode`, is read-only, and displays the structured context an agent receives. Press `g` to refresh it and `q` to close its window.

## Protocol version 1

The in-process dispatcher accepts JSON-compatible alists. A future local transport can encode the same request and response shapes without changing the semantic operations.

List operations and native Emacs feature availability. Each feature identifies
its exposed `context_section`; null means a callable facility is cataloged but
is not exposed by protocol version 1. Discovery uses constant-time function
bindings and never scans `load-path`:

```elisp
(vela-agent-handle-request
 '(("operation" . "capabilities.list")))
```

Read explicitly selected context:

```elisp
(vela-agent-handle-request
 '(("operation" . "context.snapshot")
   ("include" . ["buffer" "org"])))
```

Buffer context includes name, optional file, major mode, modified status, point, line, column, and active-region bounds. Org context uses native Org APIs to expose the current heading ID, title, level, TODO keyword, tags, outline path, and source-block name, language, and source digest. It does not return source contents implicitly.

Every context request fails closed when the source buffer exceeds 1,048,576
characters. This cap bounds native line and Org traversal, source extraction,
and hashing work on the main thread. The interface buffer uses an ordered JSON
serializer so object order follows the deterministic protocol alist order.
The `include` vector accepts each of the two supported sections at most once,
which also bounds request traversal.

Unknown operations and unknown context sections fail with `vela-agent-protocol-error`. The interface does not accept arbitrary Emacs Lisp.

## Single-thread boundary

Emacs handlers must remain short and bounded on the editor-owner thread captured
when the package loads; direct worker-thread calls fail closed. Long-running
model calls, indexing, builds, network work, and computation belong in external
workers. A later JSON-RPC bridge will need a bounded queue, asynchronous job
handles, cancellation, and short main-thread callbacks. Delayed edits must carry
buffer identity and modification-tick preconditions, and authority-bearing
operations must carry Vela approval evidence.

The current slice has no transport, queue, edits, Babel execution, tangling, exporting, shell execution, or Magit mutation. Feature metadata advertises the Emacs facilities the interface is intended to standardize; it does not claim those effects are currently authorized.

## Verification

```bash
nix develop --command just emacs-test
```

The full repository gate also runs these byte-compilation and ERT checks:

```bash
nix develop --command just verify
```
