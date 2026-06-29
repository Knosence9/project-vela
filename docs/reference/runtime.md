# Runtime compatibility notes

## Implemented in the Rust scaffold
- bare `vela` now bootstraps a runtime session instead of only printing help
- `chat` now executes a runtime turn instead of only bootstrapping a session
- runtime session bootstrap persists into `state.db`
- session rows currently store:
  - `id`
  - `title`
  - `command_name`
  - `interaction_mode`
  - `created_at`
  - `updated_at`
- `--resume <session-id-or-title>` resumes a stored session
- `--continue <name>` resumes the latest matching titled session
- bare `--continue` resumes the latest session
- interactive vs single-turn mode is derived from whether query/image input is present
- `status` reports the latest active session identity
- `gateway` now bootstraps durable gateway directories/config and can resume a gateway-specific runtime session
- `cron` now bootstraps durable scheduler config/job state and can resume a scheduler-specific runtime session

## Current runtime behavior
- bare `vela` creates an interactive chat session and appends an interactive runtime-ready assistant message when no explicit resume target is given
- `vela chat --query ...` creates a single-turn session and can call a configured local Ollama model for text turns
- configured provider-backed turns can run a bounded iterative local tool loop with approved read-only runtime tools before producing the final assistant reply
- `vela chat --image ...` can call a configured local Ollama model for first-pass provider-backed image turns
- `vela chat --query ... --checkpoints` can emit review signals and generate review candidates during live execution
- when no provider is configured, or a request cannot use provider-backed execution, query/image turns fall back to deterministic local-kernel scaffold responses
- repeated resume/continue paths update `updated_at` on the matching session row
- active-session reporting currently resolves to the latest `updated_at` row in `sessions`
- `vela gateway --start` resumes the latest `gateway` command session when one already exists
- `vela cron --start` resumes the latest `cron` command session when one already exists

## Still needed
- richer runtime state transitions beyond created/resumed shell states
- broader external provider/model execution beyond the first Ollama text/image-turn slices and bounded iterative tool loop
- session titles/naming behavior closer to upstream truth
- explicit continue semantics matching upstream lineage behavior
- turn lifecycle persistence
- session branching/compression semantics
- actual recurring job execution and restart recovery beyond durable registration
