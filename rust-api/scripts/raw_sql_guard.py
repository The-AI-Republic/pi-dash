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
* positional row reads (`.try_get(0)`, `.try_get::<T, _>(2)`,
  `Row::try_get(&row, 3)`, `.get::<T, _>(1)`, `try_get_unchecked`,
  SeaORM `try_get_by_index`), which go stale whenever a column is added or
  dropped. A bare `.get(0)` is not counted: statically it cannot be told
  apart from slice indexing.

A `// raw-sql: <reason>` (or `/* raw-sql: ... */`) comment exempts the
sites on its own line, or, when it stands alone on its line, the sites on
the next line. Text inside a string literal is not a comment.

The counts are compared with the committed allowlist
(scripts/raw_sql_allowlist.txt, `path count`). A file above its entry, or a
file with sites and no entry, fails: no new raw SQL. A file below its entry
fails too: lower the entry in the same PR, so the list only ever shrinks.

It also fails on a hand-written statement whose output list (a SELECT's
select list, the main SELECT of a WITH, or a RETURNING clause) has two
output columns with the same name (sqlx resolves a duplicated name to the LAST
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

# Qualified sqlx entry points, whatever the path prefix (`sqlx::`, `::sqlx::`,
# `crate::sqlx::`): a call to sqlx's query API by name.
RE_SQLX_QUALIFIED = re.compile(
    r"\bsqlx\s*::\s*(?:query|query_as|query_scalar|query_with|query_as_with|query_scalar_with)"
    r"\s*(?:::\s*<[^;{}]*?>\s*)?\(|"
    r"\bsqlx\s*::\s*(?:query|query_as|query_scalar)(?:_unchecked)?!\s*\(|"
    r"\bsqlx\s*::\s*raw_sql\s*\(|"
    r"\bQueryBuilder(?:\s*::\s*<[^;{}]*?>)?\s*::\s*new\s*\(")
IMPORTABLE = ("query", "query_as", "query_scalar", "query_with", "query_as_with",
              "query_scalar_with", "raw_sql")
RE_USE_SQLX = re.compile(r"\buse\s+(?:::)?(?:crate::)?sqlx::(\{[^}]*\}|[^;]+);")
RE_SEAORM_RAW = re.compile(
    r"\bStatement::from_(?:string|sql_and_values)\s*\(|\.execute_unprepared\s*\(|"
    r"\.from_raw_sql\s*\(|\.find_by_statement\s*\(|"
    r"\bExpr::cust(?:_with_values|_with_expr|_with_exprs)?\s*\(")
# Positional row reads: `.try_get(0)`, `.try_get::<T, _>(2)`, `.try_get(20 + $pos)`,
# `Row::try_get(&r, 3)`, `.get::<T, _>(1)`, `Row::get(&r, 1)`,
# `try_get_unchecked(0)`, SeaORM `try_get_by_index(0)`. A bare `.get(0)` is
# not counted: statically it is indistinguishable from slice indexing.
RE_POSITIONAL = re.compile(
    r"\.(?:try_get|try_get_unchecked|try_get_by_index)\s*(?:::\s*<[^;{}()]*?>\s*)?\(\s*\d|"
    r"\.get\s*::\s*<[^;{}()]*?>\s*\(\s*\d|"
    r"\bRow\s*::\s*(?:try_get|get|try_get_unchecked)\s*(?:::\s*<[^;{}()]*?>\s*)?"
    r"\(\s*[^,()]+,\s*\d")
RE_REASON = re.compile(r"(?://|/\*)\s*raw-sql:\s*\S")
RE_IDENT = re.compile(r"[A-Za-z_]\w*")
_SEGMENT = r'(?:"[^"]+"|[A-Za-z_]\w*)'
RE_COLUMN_REF = re.compile(
    r"(?:" + _SEGMENT + r"\s*\.\s*)*(?P<name>" + _SEGMENT + r")"
    r"(?:\s*::\s*\w+(?:\s*\(\s*\d+(?:\s*,\s*\d+)?\s*\))?(?:\[\])?)*")
# Trailing words that end an expression rather than alias it, and words
# after which a trailing identifier is an operand, not an alias.
NOT_ALIASES = frozenset({"end", "null", "true", "false", "asc", "desc", "distinct"})
OPERAND_AFTER = frozenset({
    "and", "or", "not", "is", "in", "like", "ilike", "similar", "then", "else",
    "when", "case", "between", "distinct", "from", "select", "on", "any", "all",
    "some", "exists", "over", "by", "as",
})

ALLOWLIST = Path(__file__).resolve().parent / "raw_sql_allowlist.txt"


class LineIndex:
    """Line number of a source offset, by bisecting the newline offsets."""

    def __init__(self, src):
        self.newlines = [match.start() for match in re.finditer("\n", src)]

    def line(self, pos):
        return bisect.bisect_left(self.newlines, pos)


def comment_lines(src, code, spans):
    """Per line: (the line's comment text, whether the line holds only a comment).

    lex() blanks comments in `code` and leaves strings in place, so a comment
    is the text blanked outside every string span. Text inside a string (a
    `"// raw-sql:"` literal) is not a comment.
    """
    in_string = bytearray(len(src))
    for start, end in spans:
        in_string[start:end] = b"\x01" * (end - start)
    comments, alone, offset = [], [], 0
    for line in src.split("\n"):
        end = offset + len(line)
        text = "".join(
            src[i] for i in range(offset, end)
            if code[i] != src[i] and not in_string[i])
        code_line = code[offset:end]
        comments.append(text)
        alone.append(bool(text.strip()) and not code_line.strip())
        offset = end + 1
    return comments, alone


def has_reason(comments, alone, line_no):
    """A `raw-sql:` comment on the site's own line, or alone on the line above."""
    if 0 <= line_no < len(comments) and RE_REASON.search(comments[line_no]):
        return True
    above = line_no - 1
    return 0 <= above < len(comments) and alone[above] and bool(RE_REASON.search(comments[above]))


def test_masked(code, masked, start, end):
    """True when code[start:end] was blanked as test code by mask_cfg_test."""
    original, kept = code[start:end], masked[start:end]
    if original == kept:
        return False
    return bool(original.strip()) and not kept.strip()


def sqlx_aliases(masked):
    """Bare names that call sqlx's query API in this file: imported entry
    points (`use sqlx::{query, query_as}`) and their aliases (`as q`)."""
    names = set()
    for match in RE_USE_SQLX.finditer(masked):
        for item in match.group(1).strip("{} \n").split(","):
            item = item.strip()
            parts = re.split(r"\s+as\s+", item)
            if parts[0].strip() in IMPORTABLE:
                names.add(parts[-1].strip())
    return names


def count_sites(src):
    """(sites, code, masked code, spans, file_mods) for one file."""
    code, spans = lex(src)
    masked, file_mods = mask_cfg_test(code, spans)
    comments, alone = comment_lines(src, code, spans)
    index = LineIndex(src)
    starts = [start for start, _ in spans]

    def in_string(pos):
        at = bisect.bisect_right(starts, pos) - 1
        return at >= 0 and spans[at][0] <= pos < spans[at][1]

    patterns = [RE_SQLX_QUALIFIED, RE_SEAORM_RAW, RE_POSITIONAL]
    names = sqlx_aliases(masked)
    if names:
        # A bare call to an imported entry point: not a method (`.query(`), a
        # path segment (`x::query(`) or a definition (`fn query(`).
        patterns.append(re.compile(
            r"(?<![\w:.])(?<!fn )(?:" + "|".join(map(re.escape, sorted(names)))
            + r")\s*(?:::\s*<[^;{}]*?>\s*)?\("))
    sites = 0
    for pattern in patterns:
        for match in pattern.finditer(masked):
            if in_string(match.start()):
                continue
            if has_reason(comments, alone, index.line(match.start())):
                continue
            sites += 1
    return sites, code, masked, spans, file_mods


def scan(text):
    """Yield (index, char, depth) for chars outside quoted literals.

    Single quotes delimit SQL literals; double quotes delimit identifiers,
    which are yielded (their content is part of a name) but never change the
    depth.
    """
    depth, quote = 0, None
    for index, ch in enumerate(text):
        if quote:
            if ch == quote:
                quote = None
            if quote is None and ch == '"':
                yield index, ch, depth
            continue
        if ch == "'":
            quote = "'"
            continue
        if ch == '"':
            quote = '"'
            yield index, ch, depth
            continue
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        yield index, ch, depth


def keyword_at(text, index, word):
    """True when `word` (upper case) starts at text[index] as a whole word."""
    if not text.upper().startswith(word, index):
        return False
    before = text[index - 1] if index else " "
    after = text[index + len(word)] if index + len(word) < len(text) else " "
    return not (before.isalnum() or before == "_") and not (after.isalnum() or after == "_")


def output_list(sql):
    """The top-level output list of a statement, or None.

    `SELECT ... FROM`, the main SELECT after a `WITH` list, and a top-level
    `RETURNING` clause of INSERT/UPDATE/DELETE. `IS [NOT] DISTINCT FROM`
    and `EXTRACT(x FROM y)` do not end a select list.
    """
    upper = sql.upper()
    starts_select = keyword_at(sql, 0, "SELECT")
    starts_with = keyword_at(sql, 0, "WITH")
    if not (starts_select or starts_with or "RETURNING" in upper):
        return None
    select_at = 0 if starts_select else None
    for index, _ch, depth in scan(sql):
        if depth != 0:
            continue
        if select_at is None and starts_with and keyword_at(sql, index, "SELECT"):
            select_at = index
        if keyword_at(sql, index, "RETURNING"):
            return sql[index + len("RETURNING"):].strip().rstrip(";")
        if select_at is not None and index > select_at and keyword_at(sql, index, "FROM"):
            previous = sql[:index].rstrip().split(" ")[-1].upper()
            if previous == "DISTINCT":
                continue  # IS [NOT] DISTINCT FROM
            body = sql[select_at + len("SELECT"):index].strip()
            if body.upper().startswith("DISTINCT ON"):
                close = next((i for i, c, d in scan(body) if c == ")" and d == 0), None)
                body = body[close + 1:] if close is not None else body
            elif body.upper().startswith("DISTINCT "):
                body = body[len("DISTINCT "):]
            return body
    return None


def split_top_level(text):
    parts, start = [], 0
    for index, ch, depth in scan(text):
        if ch == "," and depth == 0:
            parts.append(text[start:index])
            start = index + 1
    parts.append(text[start:])
    return parts


def column_name(item):
    """The output name of one output-list item, or None when it cannot be
    resolved statically. Quoted names keep their case; others fold to lower."""
    item = item.strip()
    if not item or item.endswith("*") or "{" in item:
        return None
    last_as = None
    for index, _ch, depth in scan(item):
        if depth == 0 and keyword_at(item, index, "AS") and item[index - 1:index] == " ":
            last_as = index
    if last_as is not None:
        candidate = item[last_as + 2:].strip()
    else:
        head, _, tail = item.rpartition(" ")
        previous = head.rstrip().split(" ")[-1].lower() if head.strip() else ""
        if (head.strip() and RE_IDENT.fullmatch(tail.strip('"'))
                and tail.strip('"').lower() not in NOT_ALIASES
                and previous not in OPERAND_AFTER
                and not re.search(r"[-+*/|=<>,~^%!]$", head.rstrip())):
            candidate = tail
        else:
            # No alias: Postgres names a plain column reference (optionally
            # cast, `a.x::text`) after its last segment, and any other
            # expression `?column?` (or its function name), which cannot clash
            # with a real column here.
            ref = RE_COLUMN_REF.fullmatch(item)
            if not ref:
                return None
            candidate = ref.group("name")
    if candidate.startswith('"') and candidate.endswith('"') and len(candidate) > 1:
        return candidate[1:-1]
    return candidate.lower() if RE_IDENT.fullmatch(candidate) else None


def unescape(literal):
    """SQL text of a Rust string literal's contents (escapes and `\\` line
    continuations resolved; raw strings pass through unchanged)."""
    text = re.sub(r"\\\n\s*", " ", literal)
    text = text.replace('\\"', '"').replace("\\n", " ").replace("\\t", " ").replace("\\r", " ")
    return re.sub(r"\s+", " ", text).strip()


def select_output_names(sql):
    """Resolvable output column names of a statement's output list, or None."""
    columns = output_list(unescape(sql))
    if columns is None:
        return None
    names = [column_name(item) for item in split_top_level(columns)]
    return [name for name in names if name]


def duplicate_selects(src, code, masked, spans, rel):
    """(key, duplicated names) for production statements with clashing names.
    Identical statements in one file share a fingerprint and are numbered,
    so a second copy of an allowlisted statement is a new entry."""
    found, seen = [], Counter()
    for start, end in spans:
        if test_masked(code, masked, start, end):
            continue
        text = src[start:end]
        names = select_output_names(text)
        if not names:
            continue
        clashes = sorted(name for name, n in Counter(names).items() if n > 1)
        if clashes:
            key = f"{rel} | {fingerprint(text)}"
            seen[key] += 1
            if seen[key] > 1:
                key = f"{key} #{seen[key]}"
            found.append((key, clashes))
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
    """Per-file counts and duplicate statements, skipping files compiled only
    for tests. `#[cfg(test)] mod foo;` in `src/lib.rs` or `src/x/mod.rs` names
    `foo.rs` / `foo/mod.rs` beside it; in `src/x.rs` it names `src/x/foo.rs`
    / `src/x/foo/mod.rs` (Rust's module resolution)."""
    scanned, skip_files = [], set()
    for path, rel in iter_production_files(root):
        src = path.read_text(encoding="utf-8")
        sites, code, masked, spans, file_mods = count_sites(src)
        base = path.parent if path.name in ("mod.rs", "lib.rs", "main.rs") else path.with_suffix("")
        for module in file_mods:
            skip_files.add(base / (module + ".rs"))
            skip_files.add(base / module / "mod.rs")
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
