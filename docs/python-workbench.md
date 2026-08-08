# Persistent Python workbench

Vela's first persistent Python workbench slice keeps the authority boundary in
Rust while preserving ordinary Python as the working language. The Rust
`vela-dev` command selects one explicit live Jupyter notebook, reads multiline
Python from standard input, invokes the caller-selected `hamelnb` adapter, and
parses its compact JSON result before printing anything.

```text
Rust command and validation
        ↓ explicit adapter + port + notebook path
hamelnb Jupyter adapter
        ↓
live Python kernel with persistent variables
```

## Usage

Start a localhost-only Jupyter server and a live notebook session through the
separately installed `hamelnb` workflow. Then execute Python through Vela:

```bash
printf '%s\n' 'values = [1, 2, 3]' \
  | nix develop --command cargo run --locked -p vela-dev -- \
      python execute "$HOME/.local/bin/hamelnb" 8888 scratch.ipynb

printf '%s\n' 'sum(values)' \
  | nix develop --command cargo run --locked -p vela-dev -- \
      python execute "$HOME/.local/bin/hamelnb" 8888 scratch.ipynb
```

The second command uses the same Python namespace as the first. Python source
is read from standard input so multiline code is not reinterpreted by the
caller's shell. The Rust adapter writes it to a permission-restricted temporary
file and gives hamelnb only that file's path through `--code-file`, avoiding
source disclosure in the child process argument list and ordinary argument-size
limits. The file is removed when the invocation completes.

Each invocation defaults to a 30-second direct-adapter timeout and captures at
most 1 MiB from each of stdout and stderr. Callers can select smaller or larger
non-zero budgets explicitly:

```bash
printf '%s\n' 'sum(values)' \
  | nix develop --command cargo run --locked -p vela-dev -- \
      python execute "$HOME/.local/bin/hamelnb" 8888 scratch.ipynb \
      --timeout-seconds 10 --max-output-bytes 262144
```

Vela drains both streams without blocking, kills and reaps a still-running
direct adapter on timeout, and rejects an overflowing stream without parsing or
printing partial JSON. Inherited output pipes cannot extend the call beyond its
deadline. The byte limit applies independently to stdout and stderr.

## Current boundary

This first slice intentionally does **not**:

- start, stop, sandbox, or authenticate Jupyter;
- grant host tools or secrets to Python;
- persist Vela provenance events or checkpoints;
- contain adapter-created descendants or impose a kernel-side deadline;
- replay exploratory state from a clean kernel; or
- treat notebook state as verified project evidence.

Until those controls land, Python has the authority of the selected kernel
environment. Use only an explicitly selected localhost notebook. Adapter
failure, malformed JSON, and non-`ok` execution status fail closed without
successful partial output. Timeout and output overflow fail the same way. See
[ADR-0107](adr/0107-bounded-python-adapter-execution.md) for the exact process
and resource authority boundary.

## Architectural direction

The Rust plane will grow toward workspace identity, policy, budgets,
provenance, checkpoints, and replay. Python remains the compact, familiar
language for exploration, parsing, tables, calculations, and retained
intermediate state. Vela should integrate the Jupyter protocol rather than
reimplement Python or invent a project-specific compute language.
