#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

command -v secretspec >/dev/null
secretspec --version

test -f secretspec.toml
grep -Fq 'name = "project-vela"' secretspec.toml
grep -Fq '[profiles.integration-test]' secretspec.toml
grep -Fq 'VELA_TEST_SECRET' secretspec.toml

fixture="$(mktemp)"
trap 'rm -f "$fixture"' EXIT
chmod 600 "$fixture"
printf '%s\n' 'VELA_TEST_SECRET=integration-fixture-not-a-credential' >"$fixture"

env -u VELA_TEST_SECRET \
  secretspec check --profile integration-test --provider "dotenv:$fixture"

# The variable is absent from the parent environment, so only SecretSpec can
# place the value in the child command's environment.
env -u VELA_TEST_SECRET \
  secretspec run --profile integration-test --provider "dotenv:$fixture" -- \
  bash -euo pipefail -c 'test -n "${VELA_TEST_SECRET:-}"'

printf 'SecretSpec integration verified without exposing secret values.\n'
