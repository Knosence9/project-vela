# ADR-0107: Bounded Python adapter execution and output capture

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1023](https://github.com/Knosence9/project-vela/issues/1023)
- **Related:** Persistent Python workbench slice in [#1021](https://github.com/Knosence9/project-vela/issues/1021)

## Context

The first persistent Python workbench slice invokes one explicit `hamelnb` process synchronously and validates its compact JSON result. Its direct child could run forever, while `Command::output` retained stdout and stderr without a bound. A malformed or compromised adapter could therefore hold the Rust caller indefinitely or exhaust its memory before Vela could reject the response.

The smallest responsible next slice belongs at the existing Rust process boundary. It does not require Vela to own Jupyter lifecycle, kernel execution policy, or a new Python protocol.

## Decision

Every `PythonExecutionRequest` carries a non-zero runtime budget and a non-zero per-stream output byte budget. The library defaults are 30 seconds and 1 MiB for each of stdout and stderr. `vela-dev python execute` uses those defaults and accepts `--timeout-seconds` and `--max-output-bytes` overrides. Zero values fail before adapter launch.

The adapter spawns the explicit direct child with piped stdout and stderr. The Rust caller drains one bounded chunk from each nonblocking stream per status poll, so a continuously writable stream cannot starve the deadline check, and retains no more than the configured bytes. If either stream crosses its budget, Vela closes the pipes and returns a typed `OutputLimitExceeded` failure; successful JSON is never parsed or emitted. If the direct child has not exited when runtime crosses its budget, Vela kills and reaps it, closes its pipe readers, and returns a typed `TimedOut` failure without parsing partial output. A direct child first observed exiting after the deadline also fails as timed out. Once the child exits within budget, Vela grants a short bounded drain interval and then parses captured stdout without waiting indefinitely for descendants to close inherited descriptors.

The byte budget applies independently to each stream. Vela does not truncate output into a successful response. Existing launch, exit, malformed-JSON, missing-status, and non-`ok` failures remain fail-closed.

## Authority boundary

This decision bounds Vela's direct adapter process and in-memory output capture. It does not:

- kill adapter-created descendants or prevent them from inheriting process resources;
- impose a deadline inside a remote or already-running Jupyter kernel;
- start, stop, sandbox, or authenticate Jupyter;
- grant host tools, filesystem, network, or secret authority to Python; or
- promote notebook state to verified evidence.

Kernel-side cancellation and process-tree containment require separate contracts because they add lifecycle and operating-system authority.

## Alternatives considered

### Keep `Command::output` and inspect output after exit

Rejected because memory is already allocated before Vela can enforce the limit, and the child can still run forever.

### Redirect output to temporary files and inspect their size after exit

Rejected because it trades unbounded memory for unbounded temporary-disk consumption during the allowed runtime.

### Own Jupyter cancellation now

Rejected because the current boundary deliberately integrates an explicit maintained adapter. Kernel request cancellation, notebook lifecycle, and direct-child process containment are distinct authorities and should not be conflated.

## Consequences

- Ordinary successful execution remains synchronous and compact.
- The Rust process retains at most the configured bytes from each adapter stream.
- Timeout and overflow produce no successful partial stdout.
- Nonblocking pipe draining is currently a Unix process-boundary implementation.
- A descendant may outlive the direct child, but inherited pipes can extend the call only through the short bounded post-exit drain interval.

## Verification

Strict RED→GREEN tests cover direct-child timeout and reap behavior, inherited-pipe handling after successful child exit, stdout and stderr overflow, continuous output, zero-limit rejection, successful in-budget execution, CLI timeout selection, and pre-launch CLI validation. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding process-group containment, kernel-side cancellation, streaming output, asymmetric stream limits, workspace policy, durable provenance, or verified replay.
