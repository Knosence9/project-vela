#!/usr/bin/env bash
set -euo pipefail

root=$(git rev-parse --show-toplevel)
verifier="$root/scripts/verify-merge-readiness"
fixtures="$root/tests/fixtures/merge-readiness"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

pass_count=0

expect_ready() {
  local fixture=$1
  local output
  output=$("$verifier" --fixture "$fixture") || {
    printf 'expected ready, got failure:\n%s\n' "$output" >&2
    exit 1
  }
  [[ $output == "READY: PR #726 head bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb satisfies exact-head merge requirements" ]]
  pass_count=$((pass_count + 1))
}

expect_blocked() {
  local name=$1
  local fixture=$2
  local expected=$3
  local output
  if output=$("$verifier" --fixture "$fixture" 2>&1); then
    printf '%s: expected failure, got success: %s\n' "$name" "$output" >&2
    exit 1
  fi
  if [[ $output != *"$expected"* ]]; then
    printf '%s: expected diagnostic containing %q, got: %s\n' "$name" "$expected" "$output" >&2
    exit 1
  fi
  pass_count=$((pass_count + 1))
}

ready="$fixtures/ready.json"
expect_ready "$ready"
expect_blocked "PR 724 false green" "$fixtures/pr-724-rate-limited.json" "latest CodeRabbit status is Review rate limited"

jq '.statuses[0].description = "Review limit reached"' "$ready" > "$tmp/limit.json"
expect_blocked "review limit" "$tmp/limit.json" "latest CodeRabbit status is Review limit reached"

jq '.statuses[0] = {"context":"CodeRabbit","state":"failure","description":"Review failed"}' "$ready" > "$tmp/failed.json"
expect_blocked "failed review" "$tmp/failed.json" "latest CodeRabbit status is Review failed"

jq '(.reviews[] | .author) = "coderabbitai-impostor"' "$ready" > "$tmp/impostor.json"
expect_blocked "impostor review" "$tmp/impostor.json" "no substantive CodeRabbit review exists"

jq '.reviews[1].body = "Review rate limited"' "$ready" > "$tmp/body-rate-limited.json"
expect_blocked "review-body rate limit" "$tmp/body-rate-limited.json" "exact-head CodeRabbit review reports Review rate limited"

jq '.review_threads.nodes[0].is_resolved = false' "$ready" > "$tmp/unresolved.json"
expect_blocked "unresolved thread" "$tmp/unresolved.json" "1 unresolved review thread"

jq '.expected_head = "cccccccccccccccccccccccccccccccccccccccc"' "$ready" > "$tmp/stale.json"
expect_blocked "stale head" "$tmp/stale.json" "expected head cccccccccccccccccccccccccccccccccccccccc does not match PR head bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

jq '.check_runs = []' "$ready" > "$tmp/no-quality.json"
expect_blocked "missing quality" "$tmp/no-quality.json" "successful Rust quality gate is missing for exact head"

jq '.reviews[1].commit_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"' "$ready" > "$tmp/uncovered.json"
expect_blocked "uncovered head" "$tmp/uncovered.json" "CodeRabbit has not submitted a review for exact head"

jq '.reviews[0].body = ""' "$ready" > "$tmp/empty.json"
expect_blocked "empty status only" "$tmp/empty.json" "no substantive CodeRabbit review exists"

jq '.pr.state = "CLOSED"' "$ready" > "$tmp/closed.json"
expect_blocked "closed PR" "$tmp/closed.json" "PR #726 is CLOSED, not OPEN"

jq '.review_threads.has_next_page = true' "$ready" > "$tmp/truncated.json"
expect_blocked "truncated threads" "$tmp/truncated.json" "review-thread data is truncated"

printf 'merge-readiness tests passed: %d\n' "$pass_count"
