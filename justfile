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

# Verify the committed secret declarations using a disposable fixture value.
secrets-check:
    bash tests/secretspec-integration.sh

# Test the fail-closed merge-readiness process boundary.
process-test:
    bash tests/merge-readiness.sh

# Byte-compile and exercise the Emacs agent interface without user configuration.
emacs-test:
    tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT; cp emacs/vela-agent-mode.el emacs/vela-workbench-ui.el "$tmp/"; emacs --batch -Q -L "$tmp" --eval '(setq byte-compile-error-on-warn t)' -f batch-byte-compile "$tmp/vela-agent-mode.el" "$tmp/vela-workbench-ui.el"
    emacs --batch -Q -L emacs -L emacs/tests -l vela-agent-mode-test -l vela-workbench-ui-test -l vela-org-source-test -f ert-run-tests-batch-and-exit

# Run a command with Vela's declared secrets resolved by SecretSpec.
with-secrets *command:
    secretspec run -- {{command}}

# Run the complete local quality gate.
verify: fmt-check check test clippy secrets-check process-test emacs-test diff-check
