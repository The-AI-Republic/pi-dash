#!/usr/bin/env python3
"""Ratchet: no new raw SQL in rust-api production code.

Direction (maintainer, 2026-11-03/04): rust-api reads and writes go through
SeaORM entities and the query builder, not hand-written SQL. Raw SQL stays
only where SeaORM cannot express a statement, and each such site says why
with a `// raw-sql: <reason>` comment on the call line or the line above it.
The playbook is .ai_design/rust_orm/reads.md.

This script counts, per production file (test code excluded the same way as
write_guard.py), the raw-SQL call sites that carry no `raw-sql:` reason:

* sqlx query entry points: `sqlx::query`, `query_as`, `query_scalar`,
  `query_with`, `query_as_with`, `query_scalar_with` (qualified or imported);
* SeaORM escape hatches that take SQL text: `Statement::from_string`,
  `Statement::from_sql_and_values`, `execute_unprepared`,
  `query_one_raw`/`query_all_raw` style `*_raw(` calls and `Expr::cust`;
* positional row reads, `.try_get(<integer>)`, which go stale whenever a
  column is added or dropped.

The counts are compared with the committed allowlist
(scripts/raw_sql_allowlist.txt, `path count`). A file above its entry, or a
file with sites and no entry, fails: no new raw SQL. A file below its entry
fails too: lower the entry in the same PR, so the list only ever shrinks.

It also fails on a hand-written SELECT whose select list has two output
columns with the same name (sqlx resolves a duplicated name to the LAST
column; that took down every work item on rustpidash, pi-dash#2850), unless
the statement is on the allowlist's `dup` section.

Usage:
    python3 scripts/raw_sql_guard.py --check    # exit 0 iff counts == allowlist
    python3 scripts/raw_sql_guard.py --seed     # print today's allowlist
    python3 scripts/raw_sql_guard.py --report   # per-crate totals (CI summary)

Pure stdlib, no DB.
"""

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from write_guard import SKIP_SEGMENTS, fingerprint, lex, mask_cfg_test  # noqa: E402

RE_SQLX = re.compile(
    r"(?<![\w:])(?:sqlx::)?(?:query|query_as|query_scalar|query_with|query_as_with|"
    r"query_scalar_with)\s*(?:::\s*<[^;{}]*?>\s*)?\(")
RE_SQLX_QUALIFIED = re.compile(r"\bsqlx::query\w*\b")
RE_SEAORM_RAW = re.compile(
    r"\bStatement::from_(?:string|sql_and_values)\s*\(|\.execute_unprepared\s*\(|"
    r"\.\w+_raw\s*\(|\bExpr::cust(?:_with_values|_with_expr|_with_exprs)?\s*\(")
RE_POSITIONAL = re.compile(r"\.try_get\s*\(\s*\d+(?:usize)?\s*\)")
RE_REASON = re.compile(r"//\s*raw-sql:\s*\S")
RE_IMPORTS_SQLX_QUERY = re.compile(
    r"\buse\s+sqlx::(?:\{[^}]*\b(?:query|query_as|query_scalar)\b[^}]*\}|"
    r"(?:query|query_as|query_scalar)\b)")
RE_SELECT_LIT = re.compile(r"^\s*SELECT\s", re.IGNORECASE)

ALLOWLIST = Path(__file__).resolve().parent / "raw_sql_allowlist.txt"


def line_of(src, pos):
    return src.count("\n", 0, pos)


def has_reason(lines, line_no):
    """A `raw-sql:` reason on the call line or the line directly above it."""
    for index in (line_no, line_no - 1):
        if 0 <= index < len(lines) and RE_REASON.search(lines[index]):
            return True
    return False


def count_sites(src):
    """Raw-SQL call sites in one file's production code, minus reasoned ones."""
    code, spans = lex(src)
    code, _ = mask_cfg_test(code, spans)
    lines = src.split("\n")
    in_string = [False] * len(code)
    for start, end in spans:
        for index in range(start, min(end, len(code))):
            in_string[index] = True
    imports_query = bool(RE_IMPORTS_SQLX_QUERY.search(code))
    sites = 0
    for pattern in (RE_SQLX, RE_SEAORM_RAW, RE_POSITIONAL):
        for match in pattern.finditer(code):
            if in_string[match.start()]:
                continue
            text = match.group(0)
            if pattern is RE_SQLX and not (RE_SQLX_QUALIFIED.match(text) or imports_query):
                # A bare `query(` that is not sqlx's (a local fn, a method).
                continue
            if has_reason(lines, line_of(src, match.start())):
                continue
            sites += 1
    return sites, code, spans


def select_output_names(sql):
    """Output column names of a SELECT's top-level select list, or None."""
    body = re.sub(r"\\\n\s*", " ", sql)
    body = re.sub(r"\s+", " ", body).strip()
    if not RE_SELECT_LIT.match(body):
        return None
    rest = body[len("SELECT "):]
    if rest.upper().startswith("DISTINCT "):
        rest = rest[len("DISTINCT "):]
    depth, start, columns, end = 0, 0, [], None
    for index, ch in enumerate(rest):
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif depth == 0 and ch == ",":
            columns.append(rest[start:index])
            start = index + 1
        elif depth == 0 and rest[index:index + 6].upper() == " FROM ":
            end = index
            break
    if end is None:
        return None
    columns.append(rest[start:end])
    names = []
    for column in columns:
        column = column.strip()
        if not column or column.endswith("*") or "{" in column:
            return None  # star selects and format! holes cannot be resolved
        alias = re.split(r"\s+AS\s+", column, flags=re.IGNORECASE)
        name = alias[-1] if len(alias) > 1 else column.split(".")[-1]
        name = name.strip().strip('"')
        if not re.fullmatch(r"[A-Za-z_]\w*", name):
            if len(alias) == 1:
                return None  # an unaliased expression: Postgres names it ?column?
        names.append(name.lower())
    return names


def duplicate_selects(src, code, spans, rel):
    """(fingerprint, duplicated names) for SELECT literals with clashing names."""
    found = []
    for start, end in spans:
        if start >= len(code) or code[start] == " ":
            continue  # blanked by mask_cfg_test (test code)
        literal = src[start:end]
        text = literal.strip('r#"')
        names = select_output_names(text)
        if not names:
            continue
        clashes = sorted(name for name, n in Counter(names).items() if n > 1)
        if clashes:
            found.append((f"{rel} | {fingerprint(text)}", clashes))
    return found


def iter_production_files(root):
    """Every crate source file outside test/fixture trees. Unlike the write
    guard this includes services/domain/: raw SQL migrates there too."""
    for path in sorted((root / "crates").rglob("*.rs")):
        rel = path.relative_to(root).as_posix()
        if any(segment in ("/" + rel + "/") for segment in SKIP_SEGMENTS):
            continue
        yield path, rel


def collect(root):
    counts, dups = {}, {}
    for path, rel in iter_production_files(root):
        src = path.read_text(encoding="utf-8")
        sites, code, spans = count_sites(src)
        if sites:
            counts[rel] = sites
        for key, clashes in duplicate_selects(src, code, spans, rel):
            dups[key] = clashes
    return counts, dups


def read_allowlist(path):
    counts, dups, section = {}, set(), "count"
    if not path.exists():
        return counts, dups
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        if line == "[dup]":
            section = "dup"
            continue
        if section == "dup":
            dups.add(line)
        else:
            rel, _, number = line.rpartition(" ")
            counts[rel.strip()] = int(number)
    return counts, dups


def render_allowlist(counts, dups):
    out = [
        "# raw_sql_guard.py allowlist: `path count` per production file.",
        "# Counts only go down: lower an entry (or delete it at 0) in the PR",
        "# that migrates sites to SeaORM. Never raise one; mark a justified",
        "# exception with `// raw-sql: <reason>` instead.",
    ]
    out += [f"{rel} {counts[rel]}" for rel in sorted(counts)]
    out += ["", "[dup]", "# SELECTs with two output columns of one name (fix, don't add)."]
    out += sorted(dups)
    return "\n".join(out) + "\n"


def report(counts):
    by_crate = Counter()
    for rel, n in counts.items():
        parts = rel.split("/")
        by_crate[parts[1] if parts[0] == "crates" and len(parts) > 1 else parts[0]] += n
    lines = [f"raw_sql_total={sum(counts.values())}", f"raw_sql_files={len(counts)}"]
    lines += [f"raw_sql_crate_{crate}={n}" for crate, n in sorted(by_crate.items())]
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--seed", action="store_true")
    group.add_argument("--report", action="store_true")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args(argv)
    counts, dups = collect(args.root)
    if args.seed:
        sys.stdout.write(render_allowlist(counts, dups))
        return 0
    if args.report:
        print(report(counts))
        return 0
    allowed, allowed_dups = read_allowlist(ALLOWLIST)
    failures = []
    for rel in sorted(set(counts) | set(allowed)):
        have, may = counts.get(rel, 0), allowed.get(rel, 0)
        if have > may:
            failures.append(
                f"NEW RAW SQL: {rel}: {have} site(s), allowlist {may}. Use SeaORM, "
                "or mark a justified exception with `// raw-sql: <reason>`.")
        elif have < may:
            failures.append(
                f"SHRINK THE ALLOWLIST: {rel}: {have} site(s), allowlist {may}. "
                f"Set it to {have}" + (" (delete the line)." if have == 0 else "."))
    for key, clashes in sorted(dups.items()):
        if key not in allowed_dups:
            failures.append(
                f"DUPLICATE OUTPUT COLUMN: {key}: {', '.join(clashes)}. sqlx resolves a "
                "duplicated name to the last column; alias one, or move the read to SeaORM.")
    for key in sorted(allowed_dups - set(dups)):
        failures.append(f"STALE [dup] ENTRY (remove it): {key}")
    print(report(counts))
    if failures:
        print("\n".join(failures))
        return 1
    print("raw_sql_guard: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
