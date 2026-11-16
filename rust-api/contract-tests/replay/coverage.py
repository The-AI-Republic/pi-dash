# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus coverage vs the route inventory (PIDASHCONV-823).

Joins the write ``(route shape, method)`` pairs from the V-01 Django
dump (``parity/route_inventory.py``, reused — never reimplemented)
with the corpus manifests (every session request declares its route)
and the Rust ownership table (``pidash-api routes --json``), and
reports covered / total. CI (``rust-api-replay.yml``) fails when a
write pair is neither covered by the corpus nor listed with a reason in
``corpus/reasons.py``, or when a listed reason goes stale.

This module is tooling, not a pytest suite (like ``parity/``): Django
is imported lazily inside the dump subprocess only.

Usage (from the repo root)::

    python rust-api/contract-tests/replay/coverage.py check --binary <pidash-api>
    python rust-api/contract-tests/replay/coverage.py write --binary <pidash-api>
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "parity"))

import route_inventory as parity  # noqa: E402 — lazy: no Django at import

from replay import effects  # noqa: E402
from replay.corpus import ALL_SESSIONS  # noqa: E402
from replay.corpus import reasons  # noqa: E402

WRITE_METHODS = effects.WRITE_METHODS

PROXIED_REASON = (
    "proxied by design: the Rust edge forwards this method to Django "
    "(routes --json lists it as proxied, no Rust handler); the byte flow "
    "is pinned by the edge/proxy contract suites, not by replay"
)
DEBUG_REASON = (
    "dev-only Django debug toolbar (__debug__/; Dead Python Code page "
    "section 5); never ported, never replayed"
)


def write_pairs(django: dict) -> dict[tuple[str, str], dict]:
    """All write (shape, method) pairs from the Django dump.

    Returns {(shape, method): {"path", "group", "view"}}. Function views
    (``any``) receive every method, so all four writes count.
    """
    pairs = {}
    for row in django["routes"]:
        methods = (
            set(parity.TRACKED_METHODS) if row["any"] else set(row["methods"])
        )
        for method in sorted(methods & WRITE_METHODS):
            pairs[(parity.shape_key(row["path"]), method)] = {
                "path": row["path"],
                "group": row["group"],
                "view": row["view"],
            }
    return pairs


def ownership(rust: dict) -> dict[tuple[str, str], str]:
    """Live ownership per (shape, method): owned | proxied | missing."""
    table: dict[tuple[str, str], dict] = {}
    for group_name, group in rust["groups"].items():
        for route in group["routes"]:
            shape = parity.shape_key(parity.normalize_rust_path(route["path"]))
            table.setdefault(shape, {}).update(
                {m: "owned" for m in route.get("owned", [])}
            )
            for method in route.get("proxied", []):
                table[shape].setdefault(method, "proxied")
    # Aliased Django shapes resolve to their Rust target (parity ALIASES).
    aliased = dict(table)
    for dj_shape, rs_shape in parity.ALIASES.items():
        key = parity.shape_key(rs_shape)
        if key in table:
            aliased.setdefault(parity.shape_key(dj_shape), table[key])
    out = {}
    for shape, methods in aliased.items():
        for method, state in methods.items():
            out[(shape, method)] = state
    return out


def covered_pairs() -> dict[tuple[str, str], list[str]]:
    """Corpus-covered (shape, method) pairs -> [session names]."""
    covered: dict[tuple[str, str], list[str]] = {}
    for sess in ALL_SESSIONS:
        for req in sess.requests:
            if req.method not in WRITE_METHODS:
                continue
            key = (parity.shape_key(req.route_template()), req.method)
            covered.setdefault(key, []).append(sess.name)
    return covered


def check_coverage(django: dict, rust_routes: dict) -> tuple[int, dict]:
    """Full coverage join. Returns (failures, report dict)."""
    pairs = write_pairs(django)
    owned = ownership(rust_routes)
    covered = covered_pairs()
    failures = 0
    report = {
        "total": len(pairs),
        "covered": {},
        "proxied": {},
        "debug": {},
        "missing_fix": {},
        "excluded": {},
        "owned_uncovered": {},
        "problems": [],
    }

    # Manifest sanity: every covered pair must exist in the inventory.
    for key, sessions in sorted(covered.items()):
        if key not in pairs:
            report["problems"].append(
                f"stale manifest: {key[1]} {key[0]} (sessions {sessions}) "
                "is not a Django write pair"
            )
            failures += 1

    for key in sorted(pairs):
        shape, method = key
        info = pairs[key]
        state = owned.get(key, "missing")
        if key in covered:
            report["covered"][key] = {"sessions": covered[key], **info, "ownership": state}
            for bucket in ("missing_fix", "excluded", "owned_uncovered"):
                if key in getattr(reasons, bucket.upper(), {}):
                    report["problems"].append(
                        f"stale reason: {method} {shape} is covered by "
                        f"{covered[key]} but still listed in {bucket}"
                    )
                    failures += 1
            continue
        if shape.startswith("/__debug__/"):
            report["debug"][key] = {"reason": DEBUG_REASON, **info}
            continue
        if state == "proxied":
            report["proxied"][key] = {"reason": PROXIED_REASON, **info}
            continue
        if state == "missing":
            entry = reasons.MISSING_FIX.get(key)
            if entry is None:
                report["problems"].append(
                    f"unlisted missing pair: {method} {shape} ({info['group']})"
                )
                failures += 1
            else:
                report["missing_fix"][key] = {"reason": entry, **info}
            continue
        # Owned but uncovered: needs an exact reason.
        entry = reasons.EXCLUDED.get(key) or reasons.OWNED_UNCOVERED.get(key)
        bucket = "excluded" if key in reasons.EXCLUDED else "owned_uncovered"
        if entry is None:
            report["problems"].append(
                f"unlisted owned pair: {method} {shape} ({info['group']})"
            )
            failures += 1
        else:
            report[bucket][key] = {"reason": entry, **info}

    # Stale exact reasons (pair vanished from the inventory).
    for bucket in ("MISSING_FIX", "EXCLUDED", "OWNED_UNCOVERED"):
        for key in getattr(reasons, bucket):
            if key not in pairs:
                report["problems"].append(
                    f"stale reason: {key[1]} {key[0]} in {bucket} "
                    "is not a Django write pair anymore"
                )
                failures += 1
    return failures, report


def print_report(report: dict) -> None:
    total = report["total"]
    covered = len(report["covered"])
    print(
        f"write routes: {total} pairs, {covered} covered "
        f"({len(report['proxied'])} proxied, {len(report['debug'])} debug, "
        f"{len(report['missing_fix'])} missing-with-fix, "
        f"{len(report['excluded'])} excluded, "
        f"{len(report['owned_uncovered'])} owned-uncovered)"
    )
    for problem in report["problems"]:
        print(f"error: {problem}")


def write_markdown(report: dict, out_path: str) -> None:
    lines = [
        "# Replay corpus coverage (PIDASHCONV-823)",
        "",
        f"Write ``(route, method)`` pairs: **{report['total']}** total, "
        f"**{len(report['covered'])}** covered by the corpus.",
        "",
        "| bucket | pairs |",
        "| --- | --- |",
        f"| covered | {len(report['covered'])} |",
        f"| proxied by design | {len(report['proxied'])} |",
        f"| debug toolbar (dev-only) | {len(report['debug'])} |",
        f"| missing with fix issue | {len(report['missing_fix'])} |",
        f"| excluded with reason | {len(report['excluded'])} |",
        f"| owned, no session yet | {len(report['owned_uncovered'])} |",
        "",
    ]
    for title, bucket in (
        ("Covered", "covered"),
        ("Proxied by design", "proxied"),
        ("Debug toolbar", "debug"),
        ("Missing with fix issue", "missing_fix"),
        ("Excluded", "excluded"),
        ("Owned, no session yet", "owned_uncovered"),
    ):
        lines.append(f"## {title}")
        lines.append("")
        rows = report[bucket]
        if not rows:
            lines.append("(none)")
            lines.append("")
            continue
        for key in sorted(rows):
            info = rows[key]
            extra = ""
            if bucket == "covered":
                extra = f" — {', '.join(info['sessions'])} ({info['ownership']})"
            elif bucket != "proxied" and bucket != "debug":
                extra = f" — {info['reason']}"
            lines.append(f"- `{key[1]} {key[0]}` ({info['group']}){extra}")
        lines.append("")
    with open(out_path, "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines))


def cmd_check(args) -> int:
    django = parity.dump_django_fresh(args.api_dir, args.settings)
    rust_routes = parity.load_rust_routes(args.binary)
    failures, report = check_coverage(django, rust_routes)
    print_report(report)
    return 1 if failures else 0


def cmd_write(args) -> int:
    django = parity.dump_django_fresh(args.api_dir, args.settings)
    rust_routes = parity.load_rust_routes(args.binary)
    _failures, report = check_coverage(django, rust_routes)
    out = args.out or os.path.join(os.path.dirname(os.path.abspath(__file__)), "COVERAGE.md")
    write_markdown(report, out)
    print_report(report)
    print(f"wrote {out}")
    return 0


def cmd_prune(args) -> int:
    """Delete stale reason entries (covered or vanished pairs).

    Batch workflow: land a session, run ``prune``, review the diff,
    re-run ``check``. Entries must stay single-line
    (``('shape', 'METHOD'): "reason",``) so pruning is exact; anything
    else fails loudly instead of being half-edited.
    """
    import ast

    django = parity.dump_django_fresh(args.api_dir, args.settings)
    rust_routes = parity.load_rust_routes(args.binary)
    _failures, report = check_coverage(django, rust_routes)
    stale = set()
    for problem in report["problems"]:
        match = re.match(
            r"stale reason: (\w+) (\S+) (is covered|in \w+ is not)",
            problem,
        )
        if match:
            stale.add((match.group(2), match.group(1)))
    if not stale:
        print("nothing to prune")
        return 0
    path = os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "corpus", "reasons.py"
    )
    with open(path, encoding="utf-8") as handle:
        lines = handle.readlines()
    kept, removed = [], 0
    for line in lines:
        stripped = line.strip()
        key = None
        if stripped.startswith("('"):
            try:
                key = ast.literal_eval(stripped.split(":", 1)[0].strip().rstrip(","))
            except (SyntaxError, ValueError):
                key = None
        if isinstance(key, tuple) and key in stale:
            removed += 1
            continue
        kept.append(line)
    if removed != len(stale):
        missing = stale - {
            ast.literal_eval(line.strip().split(":", 1)[0].strip().rstrip(","))
            for line in lines
            if line.strip().startswith("('")
        }
        print(f"refusing: {len(stale) - removed} stale keys have no single-line entry: {sorted(missing)[:5]}")
        return 1
    with open(path, "w", encoding="utf-8") as handle:
        handle.writelines(kept)
    print(f"pruned {removed} entries from corpus/reasons.py")
    return 0


def cmd_seed_reasons(args) -> int:
    """Print exact reason entries for uncovered pairs (authoring aid).

    The output seeds ``corpus/reasons.py``; the author curates every
    entry (fix-issue links, denylist evidence, batch plan) before it is
    committed. Never imported — stdout only.
    """
    django = parity.dump_django_fresh(args.api_dir, args.settings)
    rust_routes = parity.load_rust_routes(args.binary)
    pairs = write_pairs(django)
    owned = ownership(rust_routes)
    covered = covered_pairs()
    missing, owned_open = [], []
    for key in sorted(pairs):
        if key in covered:
            continue
        shape, method = key
        if shape.startswith("/__debug__/"):
            continue
        if owned.get(key) == "proxied":
            continue
        info = pairs[key]
        entry = f'    {key!r}: "...",  # {info["group"]} :: {info["view"]}'
        (missing if owned.get(key, "missing") == "missing" else owned_open).append(entry)
    print("MISSING_FIX = {")
    print("\n".join(missing))
    print("}")
    print()
    print("OWNED_UNCOVERED = {")
    print("\n".join(owned_open))
    print("}")
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description="Replay corpus coverage")
    sub = parser.add_subparsers(dest="command", required=True)
    for name, help_text in (
        ("check", "fail on any write pair without coverage or reason"),
        ("write", "regenerate COVERAGE.md"),
        ("seed-reasons", "print reason entries for uncovered pairs"),
        ("prune", "delete stale reason entries (covered or vanished)"),
    ):
        cmd = sub.add_parser(name, help=help_text)
        cmd.add_argument("--binary", default="pidash-api")
        cmd.add_argument("--api-dir", default="apps/api")
        cmd.add_argument("--settings", default="pi_dash.settings.test")
    sub.choices["write"].add_argument("--out", default="")
    args = parser.parse_args(argv)
    if args.command == "check":
        return cmd_check(args)
    if args.command == "write":
        return cmd_write(args)
    if args.command == "seed-reasons":
        return cmd_seed_reasons(args)
    if args.command == "prune":
        return cmd_prune(args)
    raise AssertionError(f"unreachable command {args.command}")  # pragma: no cover


if __name__ == "__main__":
    raise SystemExit(main())
