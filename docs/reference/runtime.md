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
- approved read-only runtime tools now include targeted internal context retrieval for memory, session history, and skill content (`view_memory`, `search_session_history`, `view_skill`) in addition to the earlier snapshot/list tools
- provider-backed turns now perform bounded reflection/retry when they see invalid tool continuations, empty provider replies, or unusable intermediate tool results
- retrieved tool context is injected back into the provider continuation path as durable tool-result artifacts, keeping context-aware reasoning auditable without allowing live mutation
- each live runtime turn now persists ordered lifecycle phases (`receive`, `deliberate`, `tool-request`, `tool-result`, `reflect`, `retry`, `respond`, `finish`, plus `failed` on error paths)
- session inspection now surfaces parsed runtime lifecycle records alongside raw messages and events
- runtime CLI turn output now reports the durable `turn_id`, lifecycle phase count, and final phase
- `vela chat --image ...` can call a configured local Ollama model for first-pass provider-backed image turns
- `vela chat --query ... --checkpoints` can emit review signals and generate review candidates during live execution
- when no provider is configured, or a request cannot use provider-backed execution, query/image turns fall back to deterministic local-kernel scaffold responses
- repeated resume/continue paths update `updated_at` on the matching session row
- active-session reporting currently resolves to the latest `updated_at` row in `sessions`
- `vela gateway --start` resumes the latest `gateway` command session when one already exists
- `vela cron --start` resumes the latest `cron` command session when one already exists

## Still needed
- richer runtime state transitions beyond created/resumed shell states at the session level
- broader external provider/model execution beyond the first Ollama text/image-turn slices, bounded iterative tool loop, bounded reflection/retry rules, and first-pass internal context retrieval tools
- session titles/naming behavior closer to upstream truth
- explicit continue semantics matching upstream lineage behavior
- lifecycle-driven branching, retry, and compression semantics built on the new per-turn phase records
- session branching/compression semantics
- actual recurring job execution and restart recovery beyond durable registration
