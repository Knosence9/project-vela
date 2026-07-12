set shell := ["bash", "-euo", "pipefail", "-c"]

# Format Rust sources.
fmt:
    cargo fmt --all

# Check formatting without changing files.
fmt-check:
    cargo fmt --all --check

# Compile every workspace target.
check:
    cargo check --workspace --all-targets --locked

# Run the workspace test suite.
test:
    cargo test --workspace --locked

# Reject every Clippy warning.
clippy:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Reject whitespace errors in tracked changes.
diff-check:
    git diff --check

# Run the complete local quality gate.
verify: fmt-check check test clippy diff-check
