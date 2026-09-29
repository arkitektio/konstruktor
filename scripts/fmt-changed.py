#!/usr/bin/env python3
"""`cargo fmt --check`, but only for the lines a change touched.

The repository carries formatting debt from before rustfmt was enforced, and reformatting it
wholesale would bury every real change in whitespace. So this checks what a change is
responsible for and nothing else: for every Rust file changed since BASE, rustfmt's version of
the file is compared with the file as it is, and a difference counts only where it overlaps a
line the change added or modified. New files are checked whole.

    python3 scripts/fmt-changed.py <base-rev>

Exits 1 and prints the offending hunks when a touched line is not formatted. Fix it with
`rustfmt --edition 2021 <file>`, then keep only the hunks in your own lines.
"""

from __future__ import annotations

import difflib
import re
import subprocess
import sys


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def changed_files(base: str) -> list[str]:
    out = git("diff", "--name-only", "--diff-filter=ACMR", base, "--", "*.rs")
    return [line for line in out.splitlines() if line]


def touched_ranges(base: str, path: str, length: int) -> list[tuple[int, int]]:
    """0-based, half-open line ranges of ``path`` (as it is now) that differ from ``base``."""
    if subprocess.run(["git", "cat-file", "-e", f"{base}:{path}"], capture_output=True).returncode != 0:
        return [(0, max(length, 1))]
    ranges = []
    for match in re.finditer(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", git("diff", "-U0", base, "--", path), re.M):
        start, count = int(match.group(1)), int(match.group(2) or 1)
        # A pure deletion (count 0) still touches the line it happened at.
        ranges.append((start - 1, start - 1 + max(count, 1)))
    return ranges


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    base = sys.argv[1]
    failed = False
    for path in changed_files(base):
        with open(path) as handle:
            current = handle.read()
        # Through stdin: given a path, rustfmt would also check every `mod` it declares.
        formatted = subprocess.run(
            ["rustfmt", "--edition", "2021", "--emit", "stdout", "--quiet"],
            input=current,
            capture_output=True,
            text=True,
        )
        if formatted.returncode != 0:
            print(f"{path}: rustfmt could not parse it\n{formatted.stderr}")
            failed = True
            continue
        ours = current.splitlines(keepends=True)
        theirs = formatted.stdout.splitlines(keepends=True)
        touched = touched_ranges(base, path, len(ours))
        matcher = difflib.SequenceMatcher(None, ours, theirs, autojunk=False)
        for tag, i1, i2, j1, j2 in matcher.get_opcodes():
            if tag == "equal":
                continue
            low, high = i1, max(i2, i1 + 1)
            if not any(low < b and a < high for a, b in touched):
                continue
            failed = True
            print(f"{path}:{i1 + 1}: not formatted")
            sys.stdout.writelines(f"-{line}" for line in ours[i1:i2])
            sys.stdout.writelines(f"+{line}" for line in theirs[j1:j2])
    if failed:
        print("\nFormat the lines above with rustfmt --edition 2021 (only your own hunks).")
        return 1
    print("Every changed line is formatted.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
