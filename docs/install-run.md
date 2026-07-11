# Vela install and run path

This document defines the current first-start run paths for the Rust Vela CLI. It is intentionally limited to local developer install/run behavior; release packaging remains a separate future slice.

## Prerequisites

- Rust toolchain with Cargo.
- Native dependencies required by the workspace build.
- If Cargo cannot discover libclang while compiling `llama-cpp-sys`, set `LIBCLANG_PATH` to your local libclang directory.

Example for this Nix-based environment:

```bash
export LIBCLANG_PATH=/nix/store/hw7m1zkrmb6mcl8m37307b5x8w4rb39s-clang-18.1.8-lib/lib
```

## Run from the workspace build output

From the repository root:

```bash
cargo build -p vela
target/debug/vela --ignore-user-config status
target/debug/vela --ignore-user-config
```

This is the fastest local developer path. It produces and runs:

```text
target/debug/vela
```

## Install into a local Cargo root

For a command-style first start without using `target/debug/vela` directly, install from the app crate path:

```bash
cargo install --path apps/vela --root "$HOME/.local/vela-rs"
export PATH="$HOME/.local/vela-rs/bin:$PATH"
vela --ignore-user-config status
vela --ignore-user-config
```

For temporary verification, use a temporary install root:

```bash
INSTALL_ROOT="$(mktemp -d /tmp/vela-install.XXXXXX)"
cargo install --path apps/vela --root "$INSTALL_ROOT" --debug
"$INSTALL_ROOT/bin/vela" --ignore-user-config status
```

The `--debug` form is useful for local smoke tests because it avoids a full optimized release build. Omit `--debug` when you want a normal release-style local install.

## Isolated first-start home

Use an isolated home when validating first-start behavior:

```bash
export VELA_HOME="$(mktemp -d /tmp/vela-first-start.XXXXXX)"
vela --ignore-user-config status
vela --ignore-user-config
```

Expected first-start shape:

- `status` reports `vela bootstrap ready`.
- A fresh home may report `config_files=0` and `resolved backend: none`.
- Bare `vela` creates an interactive runtime session and prints `Interactive Vela runtime ready`.

## Scope boundary

This is not release packaging. The current supported local paths are:

1. `cargo build -p vela` followed by `target/debug/vela ...`
2. `cargo install --path apps/vela --root <install-root>` followed by `<install-root>/bin/vela ...`

Future release packaging, installers, shell completions, or distribution-specific bundles should be handled by separate issues.
