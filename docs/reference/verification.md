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
  - session inspection parses ordered runtime lifecycle records from persisted events
- `crates/vela-runtime`
  - scheduler job persistence and dedupe
  - gateway restart continuity avoids duplicate bootstrap messages
  - scheduler restart continuity reuses the same command session and preserves registered jobs
  - chat turn execution appends an assistant response and can generate checkpoint artifacts
  - configured Ollama execution is used for text turns when provider/model settings are present
  - configured Ollama execution is used for image turns when provider/model settings are present
  - configured provider turns can request approved local tools across a bounded multi-step loop and continue with each tool result
  - configured provider turns can retrieve targeted memory, session history, and skill context through read-only runtime tools
  - runtime turns persist ordered lifecycle phases for local, provider-backed, and tool-loop execution paths
  - provider-backed turns can reflect on invalid intermediate continuations, retry within strict limits, and fall back deterministically when recovery fails
  - max-step fallback, reflection retries, and tool-loop execution preserve ordered lifecycle phase records through `finish`
- `apps/vela/tests/cli_verification.rs`
  - bare `vela` creates a runtime session visible in `vela status`
  - bare `vela` emits an interactive runtime-ready message on first creation
  - `vela chat --query ... --checkpoints` executes a runtime turn, reports lifecycle state, and produces review candidates
  - `vela chat --query ...` uses a configured Ollama provider when present
  - configured provider turns can complete a bounded multi-step runtime tool loop through the CLI while reporting lifecycle phase counts
  - configured provider turns can retrieve targeted skill context through the CLI tool loop
  - configured provider turns can recover from an invalid tool request through bounded reflection/retry in the CLI path
  - `vela chat --image ...` uses a configured Ollama provider when present
  - `vela chat --image ...` falls back to a deterministic local-kernel scaffold response when no provider is configured
  - `vela gateway --start` resumes the same gateway session
  - `vela cron` registration persists across `--show`/`--list`
  - invalid `vela cron --schedule ...` usage is rejected at CLI parse time

## Remaining gaps
- end-to-end live runtime loop behavior beyond the current local Ollama text/image-turn paths, bounded iterative tool loop, bounded reflection/retry, first-pass lifecycle persistence, and the new internal context retrieval tools
- broader review-pipeline integration from transcript -> candidate -> pending -> approval via CLI-only tests
- model/tool execution verification once runtime behavior moves beyond the current shell scaffolding
