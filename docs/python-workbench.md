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

## Current boundary

This first slice intentionally does **not**:

- start, stop, sandbox, or authenticate Jupyter;
- grant host tools or secrets to Python;
- persist Vela provenance events or checkpoints;
- bound adapter runtime or output size;
- replay exploratory state from a clean kernel; or
- treat notebook state as verified project evidence.

Until those controls land, Python has the authority of the selected kernel
environment. Use only an explicitly selected localhost notebook. Adapter
failure, malformed JSON, and non-`ok` execution status fail closed without
successful partial output.

## Architectural direction

The Rust plane will grow toward workspace identity, policy, budgets,
provenance, checkpoints, and replay. Python remains the compact, familiar
language for exploration, parsing, tables, calculations, and retained
intermediate state. Vela should integrate the Jupyter protocol rather than
reimplement Python or invent a project-specific compute language.
