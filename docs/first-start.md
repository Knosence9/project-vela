# Vela first-start quickstart

This guide gets a fresh checkout to a working Vela kernel smoke test. It uses a temporary `VELA_HOME` so the first run does not depend on your existing user config.

## Prerequisites

- Rust toolchain with Cargo.
- Native dependencies needed by the current workspace build. In this environment the build needs `LIBCLANG_PATH` so `llama-cpp-sys` can find libclang.

Example for this Nix-based environment:

```bash
export LIBCLANG_PATH=/nix/store/hw7m1zkrmb6mcl8m37307b5x8w4rb39s-clang-18.1.8-lib/lib
```

Use the libclang path appropriate for your machine if Cargo cannot discover it automatically.

## 1. Build the CLI

From the repository root:

```bash
cargo build -p vela
```

A successful build produces:

```text
target/debug/vela
```

You can also use `cargo run -p vela -- ...` for the commands below, but the examples use the built binary to make the first-start path explicit.

## 2. Create an isolated first-start home

```bash
export VELA_HOME="$(mktemp -d /tmp/vela-first-start.XXXXXX)"
```

Optional cleanup when you are done:

```bash
rm -rf "$VELA_HOME"
```

## 3. Inspect first-start readiness

```bash
target/debug/vela --ignore-user-config status
```

Expected first-start shape:

- `vela bootstrap ready` appears.
- `home=` points at your temporary `VELA_HOME`.
- `config_files=0` is normal for a fresh home.
- `resolved backend: none` is normal before provider config.
- persistence, memory, skills, reviews, and extensions directories are reported.

A missing provider config is not a startup failure. The kernel and mock provider paths are still usable for first smoke tests.

## 4. Start the kernel once

```bash
target/debug/vela --ignore-user-config
```

Expected success shape:

```text
runtime session: action=created state=finish ... title=chat interactive mode=interactive
Interactive Vela runtime ready. Session: chat interactive (...)
response route: source=runtime-kernel
lifecycle: ... last=finish
```

This proves the Rust kernel can create a durable runtime session without a configured external provider.

## 5. Run a local mock chat smoke

```bash
target/debug/vela --ignore-user-config chat \
  --provider mock \
  --query "first start smoke" \
  --yolo
```

Expected success shape:

```text
runtime session: action=created state=finish ... title=chat: first start smoke mode=single-turn
Mock provider says hi.
response route: source=runtime-mock provider=mock ...
lifecycle: ... last=finish
```

This proves the chat path can execute an end-to-end runtime turn through an in-process provider.

## 6. Start the gateway scaffold

```bash
target/debug/vela --ignore-user-config gateway --start
```

Expected success shape:

```text
gateway started: session=... action=created title=gateway-... config=$VELA_HOME/gateway/config.json
```

This proves the gateway bootstrap path can create its durable session and config surface.

## Optional: configure a real backend

The Rust scaffold currently recognizes these backend IDs in `vela status`:

- `ollama`
- `mock`
- `llamacpp`
- `embedded`

For the supported config field set, see [`docs/reference/config.md`](reference/config.md). For the current backend/provider contract, capability matrix, and local-provider safety notes, see [`docs/reference/runtime.md`](reference/runtime.md).

A minimal user config lives at:

```text
$VELA_HOME/config.yaml
```

Example shape:

```yaml
runtime:
  provider: mock
```

For local embedded inference, provide a model path:

```yaml
runtime:
  provider: embedded
  embedded_model_path: /path/to/model.gguf
```

Use `target/debug/vela --ignore-user-config status` after writing config to confirm the config source and resolved backend. Drop `--ignore-user-config` when you intentionally want `$VELA_HOME/config.yaml` to take precedence.

## Troubleshooting

- If build fails while compiling `llama-cpp-sys`, set `LIBCLANG_PATH` to your local libclang directory and rerun `cargo build -p vela`.
- If `target/debug/vela` is missing, rerun `cargo build -p vela` from the repository root.
- If `resolved backend: none` appears on a fresh home, either run the mock provider smoke above or create `$VELA_HOME/config.yaml` with a provider.
- If you want a fully isolated smoke, keep using `--ignore-user-config` and a temporary `VELA_HOME`.
