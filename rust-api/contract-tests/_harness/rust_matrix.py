#!/usr/bin/env python3
# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Matrix + gate helpers for the Rust HTTP contract workflow (PIDASHCONV-821).

The workflow (``.github/workflows/rust-api-contract-rust.yml``) shells out to
this module; it uses only the standard library and reads its inputs (the
``*.txt``/``*.md`` files next to it) by path, so it runs as a plain script
with no imports from the harness:

- ``matrix`` — print the JSON suite array for the workflow's setup job.
- ``check`` — fail unless every on-disk suite directory is either listed in
  ``HTTP_SUITES.txt`` or in the task-only set below, every ignore exists,
  and every known-failure entry names a listed suite exactly once.
- ``gate`` — implement the known-failures rule for one finished matrix
  entry (see ``cmd_gate``).

A "suite" is one directory under ``rust-api/contract-tests/`` holding
``test_*.py`` files.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

CT = Path(__file__).resolve().parent.parent
HTTP_SUITES_TXT = CT / "HTTP_SUITES.txt"
IGNORES_TXT = CT / "TASK_ORACLE_IGNORES.txt"
KNOWN_FAILURES_MD = CT / "KNOWN_RUST_FAILURES.md"

# Suite directories that never drive HTTP, hence never join the Rust matrix.
# dispatch/integrations/tasks_* are task oracles (Celery wire format + DB
# diff, covered by the worker-plane oracle and the *-rust replay workflows);
# ops drives the pidash-api ops binary via subprocess (covered by the
# rust-api-contract-tests-ops*.yml workflows). Add a directory here only
# with a justification; ``check`` fails on any other unlisted directory.
TASK_ONLY_DIRS = frozenset(
    {
        "dispatch",
        "integrations",
        "ops",
        "tasks_cleanup",
        "tasks_mail",
        "tasks_ticker",
        "tasks_webhooks",
    }
)

ENTRY_RE = re.compile(r"^-\s+`([A-Za-z0-9_]+)`:\s+(PIDASHCONV-\d+)\b")


def read_list(path: Path) -> list[str]:
    """Non-blank, non-comment lines of a ``*.txt`` list file, in order."""
    return [
        line.strip()
        for line in path.read_text().splitlines()
        if line.strip() and not line.strip().startswith("#")
    ]


def http_suites() -> list[str]:
    return read_list(HTTP_SUITES_TXT)


def on_disk_suite_dirs() -> set[str]:
    """Directories directly under contract-tests/ holding test_*.py files."""
    found = set()
    for child in CT.iterdir():
        if not child.is_dir():
            continue
        if child.name.startswith((".", "_")) or child.name == "__pycache__":
            continue
        if list(child.glob("test_*.py")):
            found.add(child.name)
    return found


def known_failures(path: Path = KNOWN_FAILURES_MD) -> dict[str, str]:
    """Parse a known-failures doc into {suite: fix-issue-id}."""
    entries: dict[str, str] = {}
    in_fence = False
    for lineno, line in enumerate(path.read_text().splitlines(), 1):
        stripped = line.strip()
        # Skip fenced blocks: the documented entry format is itself an
        # example entry and must never parse as a live one.
        if stripped.startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        match = ENTRY_RE.match(stripped)
        if not match:
            continue
        suite, issue = match.groups()
        if suite in entries:
            raise ValueError(f"{path.name}:{lineno}: duplicate entry for `{suite}`")
        entries[suite] = issue
    return entries


def cmd_matrix() -> int:
    print(json.dumps(http_suites()))
    return 0


def cmd_check() -> int:
    """Coverage + consistency gate for the matrix inputs. Fails loudly."""
    errors: list[str] = []
    listed = http_suites()
    if len(set(listed)) != len(listed):
        dupes = sorted({s for s in listed if listed.count(s) > 1})
        errors.append(f"{HTTP_SUITES_TXT.name}: duplicate suites: {', '.join(dupes)}")
    on_disk = on_disk_suite_dirs()
    for suite in sorted(set(listed) - on_disk):
        errors.append(f"{HTTP_SUITES_TXT.name}: `{suite}` has no test_*.py on disk")
    for suite in sorted(on_disk - set(listed) - TASK_ONLY_DIRS):
        errors.append(
            f"`{suite}/` holds tests but is in neither {HTTP_SUITES_TXT.name} nor "
            "the task-only set: add it to one (with justification for task-only)"
        )
    for suite in sorted(TASK_ONLY_DIRS - on_disk):
        errors.append(f"task-only set: `{suite}/` no longer exists on disk (stale exclusion)")
    for lineno, rel in enumerate(read_list(IGNORES_TXT), 1):
        target = CT / rel
        if not target.is_file():
            errors.append(f"{IGNORES_TXT.name}:{lineno}: `{rel}` does not exist")
        elif rel.split("/", 1)[0] not in listed:
            errors.append(f"{IGNORES_TXT.name}:{lineno}: `{rel}` is not under a listed suite")
    try:
        known = known_failures()
    except ValueError as exc:
        errors.append(str(exc))
        known = {}
    for suite in sorted(known):
        if suite not in listed:
            errors.append(f"{KNOWN_FAILURES_MD.name}: `{suite}` is not a listed suite")
    if errors:
        print("rust_matrix check FAILED:")
        for error in errors:
            print(f"  - {error}")
        return 1
    print(
        f"rust_matrix check ok: {len(listed)} HTTP suites, "
        f"{len(TASK_ONLY_DIRS)} task-only dirs, "
        f"{len(read_list(IGNORES_TXT))} ignored files, "
        f"{len(known)} known failures"
    )
    return 0


def gate_verdict(
    *,
    suite: str,
    django_exit: int,
    rust_exit: int,
    suites: list[str],
    known: dict[str, str],
) -> tuple[int, str]:
    """Pure verdict for one matrix entry after both pytest runs finished.

    Returns (exit_code, message). Rules:

    - The Django run is oracle validity: any nonzero exit fails, listed or
      not (the known-failures list covers Rust divergence, never Django).
    - Rust exit 0 while listed fails: the entry is stale, the fix PR must
      drop it (a green fix PR proves staleness).
    - Rust exit 1 (tests failed) while listed passes as an expected failure;
      unlisted it fails: file one fix issue per root cause and list it.
    - Any other Rust exit (2 interrupted, 3 internal error, 4 usage error,
      5 no tests collected) always fails: a broken run must never hide
      behind the list.
    """
    if suite not in suites:
        return 1, f"gate FAILED: `{suite}` is not in {HTTP_SUITES_TXT.name}"
    if django_exit != 0:
        return 1, (
            f"gate FAILED: `{suite}` exited {django_exit} against Django: "
            "the oracle itself is invalid, fix the suite or the stack first "
            "(known-failures entries never cover the Django run)"
        )
    if rust_exit == 0 and suite in known:
        return 1, (
            f"gate FAILED: `{suite}` passed against Rust but is still listed in "
            f"{KNOWN_FAILURES_MD.name} ({known[suite]}): drop the entry"
        )
    if rust_exit == 1 and suite in known:
        return 0, (
            f"gate ok: `{suite}` failed against Rust as listed in "
            f"{KNOWN_FAILURES_MD.name} ({known[suite]})"
        )
    if rust_exit == 1:
        return 1, (
            f"gate FAILED: `{suite}` failed against Rust (exit 1) and is not listed "
            f"in {KNOWN_FAILURES_MD.name}: file one fix issue per root cause "
            "(title `Rust <domain> fix: …`) and list the suite with its id"
        )
    if rust_exit == 0:
        return 0, f"gate ok: `{suite}` passed on both backends"
    return 1, (
        f"gate FAILED: `{suite}` Rust run ended with exit {rust_exit} "
        "(not a test failure: collection error, interruption, or no tests "
        f"collected): investigate, the list never covers exit {rust_exit}"
    )


def cmd_gate(*, suite: str, django_exit: int, rust_exit: int) -> int:
    """Read the matrix inputs and print the verdict for one entry."""
    code, message = gate_verdict(
        suite=suite,
        django_exit=django_exit,
        rust_exit=rust_exit,
        suites=http_suites(),
        known=known_failures(),
    )
    print(message)
    return code


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("matrix", help="print the JSON suite array for GHA")
    sub.add_parser("check", help="validate the matrix inputs")
    gate = sub.add_parser("gate", help="verdict for one finished matrix entry")
    gate.add_argument("--suite", required=True)
    gate.add_argument("--django-exit", required=True, type=int)
    gate.add_argument("--rust-exit", required=True, type=int)
    args = parser.parse_args(argv)
    if args.command == "matrix":
        return cmd_matrix()
    if args.command == "check":
        return cmd_check()
    return cmd_gate(suite=args.suite, django_exit=args.django_exit, rust_exit=args.rust_exit)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
