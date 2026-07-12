# Contributing to Project Vela

Project Vela is built through small, reviewable changes that produce both working Rust and trustworthy development evidence.

## Workflow

1. Start from an issue with scope, non-goals, acceptance criteria, and verification steps.
2. Create a short-lived branch from `main`.
3. Use test-driven development for behavior changes.
4. Run deterministic checks locally.
5. Capture useful development evidence without secrets or private conversation.
6. Open a pull request and resolve automated and human review findings.
7. Squash-merge only after required checks pass.

## Branches

Use a typed prefix:

- `feat/` — new behavior
- `fix/` — bug fixes
- `refactor/` — structural changes without intended behavior changes
- `test/` — test-only work
- `docs/` — documentation
- `ci/` — automation and quality gates
- `chore/` — maintenance

## Commits

Use Conventional Commits:

```text
feat(corpus): validate development records
fix(runtime): propagate task cancellation
refactor(memory): isolate event serialization
```

Keep commits cohesive. Pull requests are squash-merged so `main` retains one clear integration commit per change.

## Development shell

Enter the pinned project environment before running development commands:

```bash
nix develop
```

For automation and one-off commands, use `nix develop --command <command>`. Do not rely on globally installed Rust or project utilities. Add required development tools to `flake.nix`; update `flake.lock` when a flake input changes.

## Rust quality gate

Before pushing non-documentation changes, run these commands inside `nix develop`:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
git diff --check
```

New behavior should follow RED → GREEN → REFACTOR: write one failing test, observe the expected failure, implement the smallest passing change, then clean up while green.

## Pull requests

A pull request should explain:

- what changed and why
- the related issue and source-plan references
- important design choices and rejected alternatives
- ownership, lifetime, async, error, or safety implications
- exact verification performed
- development records produced, or why none were appropriate
- known limitations and follow-up work

Architecture, security, identity, permission, and destructive changes require explicit owner approval. Routine changes may auto-merge after required checks and reviews pass.

## Development evidence

Useful Vela-native records include:

- task → verified patch
- diagnostic → correction
- review finding → revision
- requirement → test
- rejected approach → rationale
- architecture decision → consequences

Failed attempts belong in clearly labeled negative examples. Only verified outcomes may be treated as high-trust examples.

Never commit:

- credentials, tokens, or private keys
- private prompts or irrelevant chat transcripts
- absolute home-directory paths
- raw unbounded logs
- personal or third-party data not needed to understand the change

Prefer concise structured findings and stable repository references.

## Architecture decisions

Record durable, consequential decisions under [`docs/adr/`](docs/adr/) using the provided template. An ADR explains context and tradeoffs; it is not a substitute for tests or implementation documentation.
