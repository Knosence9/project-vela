#!/usr/bin/env python3
import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path


def run(cmd: list[str], cwd: Path) -> str:
    result = subprocess.run(cmd, cwd=cwd, check=True, text=True, capture_output=True)
    return result.stdout


def sync_main(cwd: Path) -> None:
    status = run(["git", "status", "--porcelain"], cwd)
    if status.strip():
        raise SystemExit(
            "Refusing to sync main with local modifications present. Commit, stash, or clean the working tree first."
        )
    subprocess.run(["git", "fetch", "origin"], cwd=cwd, check=True)
    subprocess.run(["git", "checkout", "main"], cwd=cwd, check=True)
    subprocess.run(["git", "merge", "--ff-only", "origin/main"], cwd=cwd, check=True)


def ensure_bullet(text: str) -> str:
    text = text.strip()
    return text if text.startswith("- ") else f"- {text}"


def update_current_state(body: str, landed_items: list[str]) -> str:
    if not landed_items:
        return body
    pattern = re.compile(r"(## Current state\nAlready landed:\n)(.*?)(\n## Remaining execution path\n)", re.S)
    match = pattern.search(body)
    if not match:
        raise SystemExit("Could not find '## Current state' section in tracker body.")

    prefix, current_items, suffix = match.groups()
    existing = [line.rstrip() for line in current_items.splitlines() if line.strip()]
    for item in landed_items:
        bullet = ensure_bullet(item)
        if bullet not in existing:
            existing.append(bullet)
    replacement = prefix + "\n".join(existing) + "\n" + suffix
    return body[: match.start()] + replacement + body[match.end() :]


def update_next_issue(body: str, next_issue: str) -> str:
    bullet = ensure_bullet(next_issue)
    pattern = re.compile(
        r"(## Remaining execution path\nPrimary next execution issue:\n)(- .*?)(\n\nFollow-on roadmap themes after that:\n)",
        re.S,
    )
    match = pattern.search(body)
    if not match:
        raise SystemExit("Could not find 'Primary next execution issue' block in tracker body.")
    replacement = match.group(1) + bullet + match.group(3)
    return body[: match.start()] + replacement + body[match.end() :]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Advance the roadmap tracker after merging an execution PR."
    )
    parser.add_argument("--tracker", type=int, default=1, help="Tracker issue number (default: 1)")
    parser.add_argument("--next", required=True, help="Next execution issue bullet text, e.g. '#53 Implement X'")
    parser.add_argument(
        "--landed",
        action="append",
        default=[],
        help="Bullet text to append under 'Already landed'. Repeat for multiple entries.",
    )
    parser.add_argument(
        "--sync-main",
        action="store_true",
        help="Fast-forward local main to origin/main before updating the tracker.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the updated tracker body instead of editing the GitHub issue.",
    )
    args = parser.parse_args()

    cwd = Path.cwd()
    if args.sync_main:
        sync_main(cwd)

    raw = run(["gh", "issue", "view", str(args.tracker), "--json", "body"], cwd)
    body = json.loads(raw)["body"]
    body = update_current_state(body, args.landed)
    body = update_next_issue(body, args.next)

    if args.dry_run:
        sys.stdout.write(body)
        return 0

    tmp_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", prefix="tracker-body-", suffix=".md", dir=cwd, delete=False
        ) as tmp:
            tmp.write(body)
            tmp_path = Path(tmp.name)
        subprocess.run(
            ["gh", "issue", "edit", str(args.tracker), "--body-file", str(tmp_path)],
            cwd=cwd,
            check=True,
        )
    finally:
        if tmp_path is not None:
            tmp_path.unlink(missing_ok=True)

    print(f"Updated tracker issue #{args.tracker}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
