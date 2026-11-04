#!/usr/bin/env python3
"""Ratchet: no new raw SQL in rust-api production code.

Direction (maintainer, 2026-11-03/04): rust-api reads and writes go through
SeaORM entities and the query builder, not hand-written SQL. Raw SQL stays
only where SeaORM cannot express a statement, and each such site says why
with a `// raw-sql: <reason>` comment on the call line, or alone on the line
above it.
The playbook is .ai_design/rust_orm/reads.md.

This script counts, per production file (test code excluded the same way as
write_guard.py), the raw-SQL call sites that carry no `raw-sql:` reason:

* sqlx query entry points: `sqlx::query`, `query_as`, `query_scalar`,
  `query_with`, `query_as_with`, `query_scalar_with` (qualified or imported),
  the `query!` / `query_as!` / `query_scalar!` macros, `sqlx::raw_sql` and
  `QueryBuilder::new`;
* SeaORM escape hatches that take SQL text: `Statement::from_string`,
  `Statement::from_sql_and_values`, `execute_unprepared`, `from_raw_sql`,
  `find_by_statement` and `Expr::cust`;
* positional row reads, `try_get(<index>)` in any form (`.try_get(0)`,
  `.try_get::<T, _>(2)`, `Row::try_get(&row, 3)`), which go stale whenever
  a column is added or dropped.

A `// raw-sql: <reason>` comment exempts the sites on its own line, or, when
it stands alone on its line, the sites on the next line.

The counts are compared with the committed allowlist
(scripts/raw_sql_allowlist.txt, `path count`). A file above its entry, or a
file with sites and no entry, fails: no new raw SQL. A file below its entry
fails too: lower the entry in the same PR, so the list only ever shrinks.

It also fails on a hand-written SELECT whose select list has two output
columns with the same name (sqlx resolves a duplicated name to the LAST
column; that took down every work item on rustpidash, pi-dash#2850), unless
the statement is on the allowlist's `dup` section.

Usage:
    python3 scripts/raw_sql_guard.py --check    # exit 0 iff counts == allowlist,
                                                # and print the totals
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
import bisect  # noqa: E402

from write_guard import SKIP_SEGMENTS, fingerprint, lex, mask_cfg_test  # noqa: E402

RE_SQLX = re.compile(
    r"(?<![\w:])(?:sqlx::)?(?:query|query_as|query_scalar|query_with|query_as_with|"
    r"query_scalar_with)\s*(?:::\s*<[^;{}]*?>\s*)?\(")
RE_SQLX_QUALIFIED = re.compile(r"\bsqlx::query\w*\b")
RE_SQLX_OTHER = re.compile(
    r"(?<![\w:])(?:sqlx::)?(?:query|query_as|query_scalar)(?:_unchecked)?!\s*\(|"
    r"\bsqlx::raw_sql\s*\(|\bQueryBuilder(?:\s*::\s*<[^;{}]*?>)?\s*::\s*new\s*\(")
RE_SEAORM_RAW = re.compile(
    r"\bStatement::from_(?:string|sql_and_values)\s*\(|\.execute_unprepared\s*\(|"
    r"\.from_raw_sql\s*\(|\.find_by_statement\s*\(|"
    r"\bExpr::cust(?:_with_values|_with_expr|_with_exprs)?\s*\(")
# `.try_get(0)`, `.try_get::<T, _>(2)`, `.try_get(20 + $pos)`, `Row::try_get(&r, 3)`.
RE_POSITIONAL = re.compile(
    r"\.try_get\s*(?:::\s*<[^;{}()]*?>\s*)?\(\s*\d|"
    r"\bRow\s*::\s*try_get\s*(?:::\s*<[^;{}()]*?>\s*)?\(\s*[^,()]+,\s*\d")
RE_REASON = re.compile(r"//\s*raw-sql:\s*\S")
RE_REASON_ALONE = re.compile(r"^\s*//\s*raw-sql:\s*\S")
RE_IMPORTS_SQLX_QUERY = re.compile(
    r"\buse\s+sqlx::(?:\{[^}]*\b(?:query|query_as|query_scalar)\b[^}]*\}|"
    r"(?:query|query_as|query_scalar)\b)")
RE_SELECT_LIT = re.compile(r"^\s*SELECT\s", re.IGNORECASE)
RE_IDENT = re.compile(r"[A-Za-z_]\w*")
# Trailing words that end an expression rather than alias it.
NOT_ALIASES = frozenset({"end", "null", "true", "false", "asc", "desc", "distinct"})

ALLOWLIST = Path(__file__).resolve().parent / "raw_sql_allowlist.txt"


class LineIndex:
    """Line number of a source offset, by bisecting the newline offsets."""

    def __init__(self, src):
        self.newlines = [match.start() for match in re.finditer("\n", src)]

    def line(self, pos):
        return bisect.bisect_left(self.newlines, pos)


def has_reason(lines, line_no):
    """A `raw-sql:` reason on the site's own line, or alone on the line above."""
    if 0 <= line_no < len(lines) and RE_REASON.search(lines[line_no]):
        return True
    above = line_no - 1
    return 0 <= above < len(lines) and bool(RE_REASON_ALONE.match(lines[above]))


def test_masked(code, masked, start, end):
    """True when code[start:end] was blanked as test code by mask_cfg_test."""
    original, kept = code[start:end], masked[start:end]
    if original == kept:
        return False
    return bool(original.strip()) and not kept.strip()


def count_sites(src):
    """(sites, masked code, spans, file_mods) for one file's production code."""
    code, spans = lex(src)
    masked, file_mods = mask_cfg_test(code, spans)
    lines = src.split("\n")
    index = LineIndex(src)
    starts = [start for start, _ in spans]

    def in_string(pos):
        at = bisect.bisect_right(starts, pos) - 1
        return at >= 0 and spans[at][0] <= pos < spans[at][1]

    imports_query = bool(RE_IMPORTS_SQLX_QUERY.search(masked))
    sites = 0
    for pattern in (RE_SQLX, RE_SQLX_OTHER, RE_SEAORM_RAW, RE_POSITIONAL):
        for match in pattern.finditer(masked):
            if in_string(match.start()):
                continue
            if pattern is RE_SQLX and not (
                RE_SQLX_QUALIFIED.match(match.group(0)) or imports_query
            ):
                continue  # a bare `query(` that is not sqlx's (a local fn, a method)
            if has_reason(lines, index.line(match.start())):
                continue
            sites += 1
    return sites, code, masked, spans, file_mods


def split_top_level(text, separator=","):
    """Split on `separator` outside parentheses and single-quoted literals."""
    parts, depth, start, quoted = [], 0, 0, False
    for index, ch in enumerate(text):
        if ch == "'":
            quoted = not quoted
        elif quoted:
            continue
        elif ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif depth == 0 and ch == separator:
            parts.append(text[start:index])
            start = index + 1
    parts.append(text[start:])
    return parts


def top_level_alias(column):
    """The explicit alias of a select-list item, `expr AS name` or `expr name`."""
    depth, quoted, last_as = 0, False, None
    upper = column.upper()
    for index, ch in enumerate(column):
        if ch == "'":
            quoted = not quoted
        elif quoted:
            continue
        elif ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif depth == 0 and upper.startswith(" AS ", index):
            last_as = index
    if last_as is not None:
        return column[last_as + 4:].strip().strip('"')
    # `table.col name` / `f(x) name`: an implicit alias is a trailing bare
    # identifier after a space at depth 0.
    head, sep, tail = column.rstrip().rpartition(" ")
    if (
        sep
        and RE_IDENT.fullmatch(tail.strip('"'))
        and tail.strip('"').lower() not in NOT_ALIASES
        and head.strip()
    ):
        if head.rstrip()[-1:] not in ("+", "-", "*", "/", "|", "=", "<", ">", ","):
            return tail.strip('"')
    return None


def select_output_names(sql):
    """Resolvable output column names of a SELECT's select list, or None.

    Items whose name cannot be resolved statically (a `*`, an unaliased
    expression, a `format!` hole) are skipped; duplicates among the rest are
    still reported.
    """
    body = re.sub(r"\\\n\s*", " ", sql)
    body = re.sub(r"\s+", " ", body).strip()
    if not RE_SELECT_LIT.match(body):
        return None
    rest = body[len("SELECT "):]
    if rest.upper().startswith("DISTINCT "):
        rest = rest[len("DISTINCT "):]
    depth, quoted, end = 0, False, None
    for index, ch in enumerate(rest):
        if ch == "'":
            quoted = not quoted
        elif quoted:
            continue
        elif ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif depth == 0 and rest[index:index + 6].upper() == " FROM ":
            end = index
            break
    if end is None:
        return None
    names = []
    for column in split_top_level(rest[:end]):
        column = column.strip()
        if not column or column.endswith("*") or "{" in column:
            continue
        alias = top_level_alias(column)
        if alias is not None:
            name = alias
        else:
            name = column.split(".")[-1].strip().strip('"')
            if not RE_IDENT.fullmatch(name):
                continue  # an unaliased expression: Postgres names it itself
        names.append(name.lower())
    return names


def duplicate_selects(src, code, masked, spans, rel):
    """(key, duplicated names) for production SELECT literals with clashing names."""
    found = []
    for start, end in spans:
        if test_masked(code, masked, start, end):
            continue
        text = src[start:end]
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
    """Per-file counts and duplicate SELECTs, skipping files that are only
    compiled for tests (`#[cfg(test)] mod foo;` -> foo.rs, foo/mod.rs)."""
    scanned, skip_files = [], set()
    for path, rel in iter_production_files(root):
        src = path.read_text(encoding="utf-8")
        sites, code, masked, spans, file_mods = count_sites(src)
        base = path.parent if path.name in ("mod.rs", "lib.rs", "main.rs") else path.with_suffix("")
        for module in file_mods:
            for candidate in (path.parent / (module + ".rs"), path.parent / module / "mod.rs",
                              base / (module + ".rs"), base / module / "mod.rs"):
                skip_files.add(candidate)
        scanned.append((path, rel, src, sites, code, masked, spans))
    counts, dups = {}, {}
    for path, rel, src, sites, code, masked, spans in scanned:
        if path in skip_files:
            continue
        if sites:
            counts[rel] = sites
        for key, clashes in duplicate_selects(src, code, masked, spans, rel):
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
