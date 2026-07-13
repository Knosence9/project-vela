# ADR 0001: Keep secret declarations in Git and values in external providers

- **Status:** Accepted
- **Date:** 2026-07-12
- **Issue:** [#343](https://github.com/Knosence9/project-vela/issues/343)
- **Source:** [SecretSpec overview](https://www.youtube.com/watch?v=dII4uMU-5R8)

## Context

Vela will eventually require credentials for model providers and external
tools. Unversioned `.env` files hide required secret names from contributors,
drift independently of the code that consumes them, and are easy to commit by
mistake. Committing values directly is unacceptable.

## Decision

Use [SecretSpec](https://secretspec.dev/) as the developer-facing boundary
between versioned declarations and externally stored values:

- `secretspec.toml` is public, reviewed project configuration.
- Secret values live in a user-selected provider; the system keyring is the
  recommended local default.
- The Nix development shell pins the CLI used by local development and CI.
- A feature adds its declaration in the same change that first consumes it.
- Automated verification uses an ephemeral, permission-restricted dotenv
  provider file and a disposable fixture value. It must remove the file on
  exit, start the child without that variable in its parent environment, avoid
  developer keyrings, and never print values.
- Vela-native corpus records may name a declaration but must never contain a
  resolved value.

The initial default profile is deliberately empty because the bootstrap CLI
does not consume credentials yet. This avoids inventing requirements before a
runtime boundary exists.

## Alternatives considered

### Reimplement secret storage in Vela

Rejected. Credential storage is security-sensitive, provider-specific work and
is not part of Vela's differentiating kernel. SecretSpec already supplies the
declarative contract, providers, profiles, injection, and audit behavior.

### Continue with `.env` files

Rejected. They keep values in the repository directory and leave declarations
unversioned. `.env` remains ignored as defense in depth, not as the preferred
provider.

### Couple `vela-dev` to the SecretSpec Rust SDK now

Deferred. The current bootstrap crate has no credential-consuming behavior.
The CLI boundary gives immediate, backwards-compatible command injection
without prematurely fixing the runtime API.

## Consequences

- Contributors can discover secret requirements from Git without seeing values.
- Local setup requires selecting and authenticating a SecretSpec provider.
- Commands that need credentials must run through `secretspec run` (or a future
  typed SDK boundary).
- Declaration changes become security-relevant review points.
- SecretSpec availability is part of the pinned Nix development environment.

## Verification

`tests/secretspec-integration.sh` checks the committed declaration, resolves a
disposable value from a temporary provider file, unsets the variable from the
parent environment, and runs a child command that requires SecretSpec to inject
it without displaying it. `just verify` runs this check with the rest of the
repository quality gate.
