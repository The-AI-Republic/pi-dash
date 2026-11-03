#!/usr/bin/env python3
"""Generate the one shared HTML5 entity table (PIDASHCONV-1553).

Reads CPython's live `html.entities.html5`, `html._invalid_charrefs` and
`html._invalid_codepoints` and emits:

* `crates/types/src/html_entities_generated.rs` — the sorted tables backing
  `pidash_types::html_entities` (the only copy in the tree),
* `fixtures/html_entities/cpython_unescape.json` — the committed CPython
  record the shared unit test verifies against: every `&key` input with
  its `html.unescape` output, plus both invalid tables.

Pure stdlib. Run from anywhere; paths resolve from this script:

    python3 rust-api/scripts/gen_html_entities.py            # regenerate
    python3 rust-api/scripts/gen_html_entities.py --check    # verify no drift

Self-checks (fail loudly, never emit a bad table): every key is ASCII,
every value is valid Unicode, the tables are byte-sorted with no
duplicate keys, every `&key` round-trips through the live
`html.unescape` to its table value, the bare-name fold (trailing `;`
stripped, first-wins) is conflict-free so `lookup_bare_entity` needs
no second table, and every C1 remap is a single char.
"""

import argparse
import html
import html.entities
import json
import platform
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUST_OUT = HERE.parent / "crates/types/src/html_entities_generated.rs"
JSON_OUT = HERE.parent / "fixtures/html_entities/cpython_unescape.json"


def rust_escape(value):
    """Escape one replacement string for a Rust `"..."` literal (pure ASCII)."""
    out = []
    for ch in value:
        o = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif 0x20 <= o <= 0x7E:
            out.append(ch)
        else:
            out.append("\\u{%x}" % o)
    return "".join(out)


def fuse_ranges(points):
    """Fuse a sorted codepoint set into inclusive (lo, hi) ranges."""
    ranges = []
    start = prev = None
    for point in sorted(points):
        if start is None:
            start = prev = point
        elif point == prev + 1:
            prev = point
        else:
            ranges.append((start, prev))
            start = prev = point
    if start is not None:
        ranges.append((start, prev))
    return ranges


def generate():
    version = platform.python_version()
    table = dict(html.entities.html5)
    charrefs = dict(html._invalid_charrefs)
    codepoints = set(html._invalid_codepoints)

    # Self-checks.
    assert all(k.isascii() for k in table), "non-ASCII html5 key"
    assert all(v == v.encode("utf-8").decode("utf-8") for v in table.values())
    keys = sorted(table)
    assert len(keys) == len(table), "duplicate html5 key"
    assert keys == sorted(keys, key=lambda k: k.encode("utf-8")), "not byte-sorted"
    for key, value in table.items():
        assert html.unescape("&" + key) == value, f"no round-trip: {key!r}"
    folded = {}
    for key in keys:
        bare = key[:-1] if key.endswith(";") else key
        assert folded.setdefault(bare, table[key]) == table[key], (
            f"bare-name conflict: {bare!r}"
        )
    assert all(len(charrefs[n]) == 1 for n in range(0x80, 0xA0)), "multi-char C1"

    header = (
        "//! Generated HTML5 entity tables — DO NOT HAND-EDIT.\n"
        "//!\n"
        f"//! Source: CPython {version} `html.entities.html5` ({len(table)} entries),\n"
        f"//! `html._invalid_charrefs` ({len(charrefs)} entries) and\n"
        f"//! `html._invalid_codepoints` ({len(codepoints)} entries),\n"
        "//! emitted by `rust-api/scripts/gen_html_entities.py`.\n"
        "//! The generator self-checked: every `&key` round-trips through the\n"
        "//! live `html.unescape` to its table value, the bare-name fold is\n"
        "//! conflict-free, and every C1 remap is a single char.\n"
    )
    lines = [header]
    lines.append("/// `(name, replacement)` sorted by name for binary search.")
    lines.append("/// Keys carry the exact `html5` spelling (trailing `;` only")
    lines.append("/// where the table has it — the legacy no-semicolon forms")
    lines.append("/// are real entries).")
    lines.append("pub static HTML5_ENTITIES: &[(&str, &str)] = &[")
    for key in keys:
        lines.append(f'    ("{key}", "{rust_escape(table[key])}"),')
    lines.append("];")
    lines.append("")
    lines.append("/// WHATWG numeric-ref remaps (`html._invalid_charrefs`), by value.")
    lines.append("pub static INVALID_CHARREFS: &[(u32, &str)] = &[")
    for num in sorted(charrefs):
        lines.append(f"    (0x{num:X}, \"{rust_escape(charrefs[num])}\"),")
    lines.append("];")
    lines.append("")
    lines.append("/// Numeric values that decode to the empty string")
    lines.append("/// (`html._invalid_codepoints`), inclusive ranges.")
    lines.append("pub static INVALID_CODEPOINT_RANGES: &[(u32, u32)] = &[")
    for lo, hi in fuse_ranges(codepoints):
        lines.append(f"    (0x{lo:X}, 0x{hi:X}),")
    lines.append("];")
    lines.append("")
    rust = "\n".join(lines)

    dump = {
        "cpython": version,
        "generator": "rust-api/scripts/gen_html_entities.py",
        "cases": {("&" + key): html.unescape("&" + key) for key in keys},
        "invalid_charrefs": {str(num): charrefs[num] for num in sorted(charrefs)},
        "invalid_codepoints": sorted(codepoints),
    }
    dump_text = json.dumps(dump, ensure_ascii=True, indent=2) + "\n"
    return rust, dump_text


def main(argv):
    check = len(argv) == 2 and argv[1] == "--check"
    if len(argv) != 1 and not check:
        print(f"usage: {Path(argv[0]).name} [--check]", file=sys.stderr)
        return 2
    rust, dump_text = generate()
    if check:
        ok = True
        for path, want in ((RUST_OUT, rust), (JSON_OUT, dump_text)):
            try:
                have = path.read_text(encoding="utf-8")
            except FileNotFoundError:
                have = None
            if have != want:
                print(f"drift: {path} differs from generator output", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    JSON_OUT.parent.mkdir(parents=True, exist_ok=True)
    RUST_OUT.write_text(rust, encoding="utf-8")
    JSON_OUT.write_text(dump_text, encoding="utf-8")
    print(f"wrote {RUST_OUT} and {JSON_OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
