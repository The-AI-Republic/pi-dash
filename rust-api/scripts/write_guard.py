#!/usr/bin/env python3
"""Ratchet: fail when Rust code writes ORM-covered tables outside domain services.

Design D7 (.ai_design/rust_orm/design.md): the tables in section 4 are written
only from pidash_services::domain (plus test fixtures). This script lists every
raw-SQL INSERT/UPDATE/DELETE and every sea-query write builder against a
covered table outside the allowed module, and compares the set with the
committed allowlist. A hit not on the list fails; an allowlist entry with no
hit fails too, so the list must shrink in the same PR that removes a site.

Entries are `path | OP table | fingerprint`, where the fingerprint is a hash of
the normalized statement -- never a line number -- so unrelated edits elsewhere
in the file do not churn the list.

Only statements that name a covered table are hits. Writes whose table is a
runtime value (`format!("UPDATE {t} ...", t)`, `Query::update().table(var)`,
table-agnostic helpers such as soft-delete or m2m-insert builders) cannot be
attributed statically and are not listed; listing them would also make the
ORM-15 empty-allowlist gate unreachable, since generic helpers never migrate.

Usage:
    python3 scripts/write_guard.py --check   # exit 0 iff hits == allowlist
    python3 scripts/write_guard.py --seed    # print today's hits to stdout

Pure stdlib, no DB. Runs in CI (rust-api-write-guard.yml) and locally.
"""

import argparse
import hashlib
import re
import sys
from pathlib import Path

# Design section 4, every table in every aggregate row. issues.workpad is a
# column of issues, covered by matching UPDATE issues.
COVERED_TABLES = frozenset({
    "issues",
    "issue_sequences",
    "issue_assignees",
    "issue_comments",
    "descriptions",
    "issue_relations",
    "users",
    "profiles",
    "accounts",
    "user_notification_preferences",
    "workspaces",
    "workspace_members",
    "projects",
    "project_members",
    "states",
    "api_tokens",
    "machine_token",
    "runner_session",
    "assistant_user_llm_config",
    "assistant_message",
})

ALLOWED_DIR = "crates/services/src/domain/"
# Path segments whose whole subtree is exempt (unit/integration/contract
# tests and fixtures; contract-tests/ and fixtures/ live outside crates/ but
# are listed so the rule survives a wider scan root).
SKIP_SEGMENTS = ("/tests/", "/contract-tests/", "/fixtures/", "/target/")

RE_INSERT = re.compile(
    r"\binsert\s+into\s+\\?\"?([A-Za-z_]\w*)\b", re.IGNORECASE)
RE_UPDATE = re.compile(
    r"\bupdate\s+(?:only\s+)?\\?\"?([A-Za-z_]\w*)\b", re.IGNORECASE)
RE_DELETE = re.compile(
    r"\bdelete\s+from\s+(?:only\s+)?\\?\"?([A-Za-z_]\w*)\b",
    re.IGNORECASE)
RE_QUERY_CTOR = re.compile(r"Query::(insert|update|delete)\s*\(\s*\)")
RE_QUERY_ANY_CTOR = re.compile(r"Query::\w+\s*\(\s*\)")
RE_TABLE_CALL = re.compile(r"\.(into_table|table|from_table)\s*\(")
RE_CFG_TEST = re.compile(r"#\[\s*cfg\s*\(\s*test\s*\)\s*\]")
RE_ATTR = re.compile(r"#\[(?:[^\[\]]|\[[^\]]*\])*\]")
RE_MOD = re.compile(r"(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+([A-Za-z_]\w*)\s*([;{])")
RE_STR_CONST = re.compile(
    r"(?:pub(?:\s*\([^)]*\))?\s+)?const\s+([A-Z][A-Z0-9_]*|TABLE)"
    r"\s*:\s*&(?:'\w+\s+)?str\s*=\s*\"([^\"]+)\"\s*;")


def lex(src):
    """Split Rust source into code with comments blanked plus string spans.

    Returns (code, spans) where code has every comment replaced by spaces
    (newlines preserved, so offsets match src) and spans is a list of
    (start, end) offsets of string-literal *contents* (quotes excluded).
    Handles // and nested /* */ comments, "...", r"..." / r#"..."# forms,
    b"..." / br"..." and char literals (lifetimes left alone).
    """
    n = len(src)
    out = list(src)
    spans = []
    i = 0

    def blank(a, b):
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        # Raw strings: [b]r#*"..."*# (prefix scan without slicing src)
        rpos = -1
        if c == "r":
            rpos = i
        elif c == "b" and nxt == "r":
            rpos = i + 1
        if rpos != -1:
            h = rpos + 1
            while h < n and src[h] == "#":
                h += 1
            if h < n and src[h] == "\"":
                hashes = h - rpos - 1
                close = "\"" + "#" * hashes
                start_content = h + 1
                j = src.find(close, start_content)
                if j == -1:
                    j = n - len(close)
                spans.append((start_content, j))
                i = j + len(close)
                continue
        # else fall through: a bare 'r'/'b' is ordinary code
        if c == "b" and nxt == "\"":
            j = i + 2
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == "\"":
                    break
                j += 1
            spans.append((i + 2, j))
            i = min(j + 1, n)
            continue
        if c == "b" and nxt == "'":
            j = src.find("'", i + 2)
            i = (j + 1) if j != -1 else n
            continue
        if c == "\"":
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == "\"":
                    break
                if src[j] == "\n":
                    break  # unterminated; bail rather than eating the file
                j += 1
            spans.append((i + 1, j))
            i = min(j + 1, n)
            continue
        if c == "'":
            # Char literal 'x' / '\n' / '\u{...}'; otherwise a lifetime.
            if nxt == "\\":
                j = src.find("'", i + 2)
                i = (j + 1) if j != -1 else n
                continue
            if i + 2 < n and src[i + 2] == "'":
                i += 3
                continue
            i += 1
            continue
        if c == "/" and nxt == "/":
            j = src.find("\n", i)
            if j == -1:
                j = n
            blank(i, j)
            i = j
            continue
        if c == "/" and nxt == "*":
            depth = 1
            j = i + 2
            while j < n and depth:
                if src[j] == "/" and j + 1 < n and src[j + 1] == "*":
                    depth += 1
                    j += 2
                elif src[j] == "*" and j + 1 < n and src[j + 1] == "/":
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
            continue
        i += 1
    return "".join(out), spans


def mask_cfg_test(code, spans=()):
    """Blank #[cfg(test)] items; return (masked_code, file_module_names).

    file_module_names holds `mod foo;` names declared under cfg(test), whose
    sibling foo.rs / foo/mod.rs files are exempt as well.
    """
    out = list(code)
    file_mods = []
    span_list = sorted(spans)

    def blank(a, b):
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    def skip_ws_attrs(j):
        while True:
            while j < len(code) and code[j] in " \t\r\n":
                j += 1
            m = RE_ATTR.match(code, j)
            if not m:
                return j
            j = m.end()

    def match_brace(j):
        depth = 0
        in_str = None  # comments are already blanked; strings are intact
        k = j
        si = 0
        while k < len(code):
            while si < len(span_list) and span_list[si][1] <= k:
                si += 1
            if si < len(span_list) and span_list[si][0] <= k < span_list[si][1]:
                k = span_list[si][1]
                continue
            ch = code[k]
            if in_str == "\"":
                if ch == "\\":
                    k += 2
                    continue
                if ch == "\"":
                    in_str = None
                k += 1
                continue
            if ch == "\"":
                in_str = "\""
                k += 1
                continue
            if ch == "'":
                # Char literal ('x', '\n', '\u{...}', '\"'); else a lifetime.
                nxt = code[k + 1] if k + 1 < len(code) else ""
                if nxt == "\\":
                    e = code.find("'", k + 2)
                    k = (e + 1) if e != -1 else len(code)
                    continue
                if k + 2 < len(code) and code[k + 2] == "'":
                    k += 3
                    continue
                k += 1
                continue
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return k + 1
            k += 1
        return len(code)

    for m in RE_CFG_TEST.finditer(code):
        j = skip_ws_attrs(m.end())
        mm = RE_MOD.match(code, j)
        if mm:
            if mm.group(2) == ";":
                file_mods.append(mm.group(1))
                blank(m.start(), mm.end())
            else:
                blank(m.start(), match_brace(mm.end() - 1))
            continue
        # cfg(test) on a fn/struct/...: blank to the item end.
        brace = code.find("{", j)
        semi = code.find(";", j)
        if brace != -1 and (semi == -1 or brace < semi):
            blank(m.start(), match_brace(brace))
        elif semi != -1:
            blank(m.start(), semi + 1)
    return "".join(out), file_mods


def normalize(text):
    return " ".join(text.split()).lower()


def fingerprint(text):
    return hashlib.sha1(normalize(text).encode("utf-8")).hexdigest()[:12]


def enclosing_span(spans, pos):
    for a, b in spans:
        if a <= pos < b:
            return (a, b)
    return None


def collect_str_consts(code):
    """Map bare CONST_NAME -> set of &str values across the file."""
    found = {}
    for m in RE_STR_CONST.finditer(code):
        found.setdefault(m.group(1), set()).add(m.group(2))
    return found


def split_args(argtext):
    """Split top-level comma-separated args (parens/strings aware)."""
    args, depth, cur, instr, esc = [], 0, [], False, False
    for ch in argtext:
        if instr:
            cur.append(ch)
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == "\"":
                instr = False
            continue
        if ch == "\"":
            instr = True
            cur.append(ch)
        elif ch in "([":
            depth += 1
            cur.append(ch)
        elif ch in ")]":
            depth -= 1
            cur.append(ch)
        elif ch == "," and depth == 0:
            args.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    args.append("".join(cur))
    return args


def balanced_call_end(code, open_paren):
    """Offset just past the ')' matching code[open_paren] == '('."""
    depth, k, instr, esc = 0, open_paren, False, False
    while k < len(code):
        ch = code[k]
        if instr:
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == "\"":
                instr = False
            k += 1
            continue
        if ch == "\"":
            instr = True
        elif ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return k + 1
        k += 1
    return len(code)


def resolve_table(expr, consts):
    """Resolve a sea-query table argument to a table name, or None if dynamic.

    Handles "lit", Alias::new(..), .to_owned()/.clone()/etc wrappers, &refs
    and &str const paths (bare name must map to exactly one value).
    """
    e = expr.strip()
    if e.startswith("&"):
        e = e[1:].strip()
    while True:
        m = re.match(r"^(?:Alias|DynIden)::new\((.*)\)$", e, re.DOTALL)
        if m:
            e = m.group(1).strip()
            continue
        m = re.match(r"^(.*)\.(?:to_owned|to_string|clone|as_str|into)\(\)$",
                     e, re.DOTALL)
        if m:
            e = m.group(1).strip()
            continue
        break
    m = re.match(r"^\"([^\"]+)\"$", e)
    if m:
        return m.group(1)
    m = re.match(r"^([A-Za-z_][\w:]*)$", e)
    if m:
        bare = m.group(1).split("::")[-1]
        vals = consts.get(bare)
        if vals and len(vals) == 1:
            return next(iter(vals))
    return None


def scan_sql(code, spans, rel):
    """Yield (kind, table, statement_text) for raw-SQL writes."""
    for regex, kind in ((RE_INSERT, "INSERT"),
                        (RE_UPDATE, "UPDATE"),
                        (RE_DELETE, "DELETE")):
        for m in regex.finditer(code):
            table = m.group(1).lower()
            if table not in COVERED_TABLES:
                continue
            span = enclosing_span(spans, m.start())
            if span is not None:
                stmt = code[span[0]:span[1]]
            else:  # SQL-looking text outside a string; window fallback
                stmt = code[max(0, m.start() - 40):m.end() + 160]
            yield kind, table, stmt, "sql"


def scan_seaquery(code, rel, consts):
    """Yield (kind, table|None, builder_text) for Query:: write builders."""
    for m in RE_QUERY_CTOR.finditer(code):
        kind = m.group(1).upper()
        window = code[m.end():m.end() + 3000]
        # The first table call belongs to this builder unless another
        # Query:: constructor starts first (then this builder sets its
        # table dynamically or not at all -- do not cross-pair).
        tcall = RE_TABLE_CALL.search(window)
        ctor = RE_QUERY_ANY_CTOR.search(window)
        if tcall is None or (ctor is not None
                             and ctor.start() < tcall.start()):
            continue
        open_paren = m.end() + tcall.end() - 1
        close = balanced_call_end(code, open_paren)
        argtext = code[open_paren + 1:close - 1]
        first = split_args(argtext)[0]
        table = resolve_table(first, consts)
        if table is None:
            continue  # dynamic table: unattributable, not a hit
        table = table.strip("\"").lower()
        if table not in COVERED_TABLES:
            continue
        yield kind, table, code[m.start():close], "sea-query"


def iter_rs_files(root):
    crates = root / "crates"
    if not crates.is_dir():
        print("error: no crates/ under %s" % root, file=sys.stderr)
        sys.exit(2)
    for path in sorted(crates.rglob("*.rs")):
        rel = path.relative_to(root).as_posix()
        if rel.startswith(ALLOWED_DIR):
            continue
        if any(seg in ("/" + rel + "/") for seg in SKIP_SEGMENTS):
            continue
        yield path, rel


def collect_hits(root):
    # Pass 1: mask test code, gather &str consts for sea-query resolution.
    files = []
    consts = {}
    skip_files = set()
    for path, rel in iter_rs_files(root):
        try:
            src = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError) as exc:
            print("warning: skipping %s (%s)" % (rel, exc), file=sys.stderr)
            continue
        code, spans = lex(src)
        masked, file_mods = mask_cfg_test(code, spans)
        for mod in file_mods:
            skip_files.add(path.parent / (mod + ".rs"))
            skip_files.add(path.parent / mod / "mod.rs")
        for name, vals in collect_str_consts(masked).items():
            consts.setdefault(name, set()).update(vals)
        files.append((path, rel, masked, spans))
    # Pass 2: scan.
    hits = []
    for path, rel, masked, spans in files:
        if path in skip_files:
            continue
        for kind, table, stmt, origin in scan_sql(masked, spans, rel):
            shown = table if table is not None else "?"
            hits.append("%s | %s %s | %s | %s"
                        % (rel, kind, shown, fingerprint(stmt), origin))
        for kind, table, stmt, origin in scan_seaquery(masked, rel, consts):
            hits.append("%s | %s %s | %s | %s"
                        % (rel, kind, table, fingerprint(stmt), origin))
    # Identical statements in one file share a fingerprint; number them so
    # every line is unique and removals still shrink the list.
    seen = {}
    deduped = []
    for line in sorted(hits):
        seen[line] = seen.get(line, 0) + 1
        if seen[line] > 1:
            line = "%s #%d" % (line, seen[line])
        deduped.append(line)
    return deduped


def read_allowlist(path):
    if not path.exists():
        return []
    lines = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line and not line.startswith("#"):
            lines.append(line)
    return lines


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--root", default=None,
                    help="rust-api directory (default: script's parent)")
    ap.add_argument("--allowlist", default=None,
                    help="allowlist path (default: <root>/scripts/...)")
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--seed", action="store_true")
    args = ap.parse_args(argv)

    root = Path(args.root) if args.root else Path(__file__).resolve().parent.parent
    allow_path = (Path(args.allowlist) if args.allowlist
                  else root / "scripts" / "write_guard_allowlist.txt")
    hits = collect_hits(root)

    if args.seed:
        for line in hits:
            print(line)
        return 0

    allowed = read_allowlist(allow_path)
    new = sorted(set(hits) - set(allowed))
    stale = sorted(set(allowed) - set(hits))
    print("write_guard: %d current site(s), %d allowlisted"
          % (len(hits), len(allowed)))
    if not new and not stale:
        print("write_guard: OK -- no new writes to covered tables")
        return 0
    for line in new:
        print("NEW SITE (add only by allowlisting, or move to domain/): %s"
              % line)
    for line in stale:
        print("STALE ENTRY (remove from the allowlist): %s" % line)
    return 1


if __name__ == "__main__":
    sys.exit(main())
