# Verification notes

## Goal
Provide executable proof for Vela kernel behaviors so PRs do not rely mainly on manual CLI transcripts.

## Current automated coverage
- `crates/vela-config`
  - invalid/unreadable user config falls back to project config
  - runtime provider/model/base-url settings load from config
- `crates/vela-memory`
  - durable memory format round-trip
  - safe legacy migration detection
  - duplicate staged add rejection
  - stale replace/remove approval diagnostics
- `crates/vela-skills`
  - conflicting staged skill actions are rejected
- `crates/vela-review`
  - duplicate candidate generation is deduplicated across repeat passes
- `crates/vela-state`
  - command session reuse is durable
  - explicit event targeting stays attached to the requested session
- `crates/vela-runtime`
  - scheduler job persistence and dedupe
  - gateway restart continuity avoids duplicate bootstrap messages
  - scheduler restart continuity reuses the same command session and preserves registered jobs
  - chat turn execution appends an assistant response and can generate checkpoint artifacts
  - configured Ollama execution is used for text turns when provider/model settings are present
  - configured Ollama execution is used for image turns when provider/model settings are present
- `apps/vela/tests/cli_verification.rs`
  - bare `vela` creates a runtime session visible in `vela status`
  - bare `vela` emits an interactive runtime-ready message on first creation
  - `vela chat --query ... --checkpoints` executes a runtime turn and produces review candidates
  - `vela chat --query ...` uses a configured Ollama provider when present
  - `vela chat --image ...` uses a configured Ollama provider when present
  - `vela chat --image ...` still produces a runtime response when no provider is configured
  - `vela gateway --start` resumes the same gateway session
  - `vela cron` registration persists across `--show`/`--list`
  - invalid `vela cron --schedule ...` usage is rejected at CLI parse time

## Remaining gaps
- end-to-end live runtime loop behavior beyond the first local Ollama text/image-turn paths
- broader review-pipeline integration from transcript -> candidate -> pending -> approval via CLI-only tests
- model/tool execution verification once runtime behavior moves beyond the current shell scaffolding
