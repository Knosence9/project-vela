# Runtime compatibility notes

## Implemented in the Rust scaffold
- bare `vela` now bootstraps a runtime session instead of only printing help
- `chat` also bootstraps a runtime session
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

## Current runtime behavior
- bare `vela` creates an interactive chat session when no explicit resume target is given
- `vela chat --query ...` creates a single-turn session
- repeated resume/continue paths update `updated_at` on the matching session row
- active-session reporting currently resolves to the latest `updated_at` row in `sessions`

## Still needed
- richer runtime state transitions beyond created/resumed shell states
- session titles/naming behavior closer to upstream truth
- explicit continue semantics matching upstream lineage behavior
- transcript persistence
- turn lifecycle persistence
- session branching/compression semantics
