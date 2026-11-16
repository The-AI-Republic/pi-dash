"""PIDASHCONV-1280: link-crawler title decoding oracle pins.

Pins the Django-observed ``<title>`` decodings the Rust crawler must
reproduce byte for byte (entities + charset). Each test replicates
exactly ``apps/api/pi_dash/bgtasks/work_item_link_task.py:141-143``::

    soup = BeautifulSoup(response.content, "html.parser")
    title = title_tag.get_text().strip() if title_tag else None

with ``response.content`` as raw response bytes. No Django import, no
worker, no network: ``bs4`` is the only dependency beyond pytest.

Hermetic by necessity, not by preference. The crawl task's SSRF guard
(``validate_url_ip``) rejects every loopback/private address, so no
live-crawl oracle can serve probe pages to the workers on an arbitrary
runner — the existing oracle crawl tests pin only the fallback shape
for the same reason. The Rust side of this contract is pinned by unit
tests in ``crates/services/src/tasks_webhooks/link_crawl.rs`` asserting
the same literals through ``crawl()`` with a stub transport (they fail
on pre-fix rust-dev); the full worker oracle was additionally verified
manually on both backends (see the PIDASHCONV-1280 workpad).

Manual full-oracle procedure (needs a guard-routable local address,
e.g. a Tailscale 100.x or a locally assigned global IPv6 — both pass
the Django and Rust guards): serve the two probe bodies below over
plain HTTP on that address, seed an ``issue_links`` row pointing at
each URL, publish ``pi_dash.bgtasks.work_item_link_task.
crawl_work_item_link_title`` with ``args=[link_id, url]`` through
``_harness.celery_wire``, and assert ``metadata["title"]`` equals the
literals pinned here — once with the Django worker draining the
queue, once with the Rust worker.
"""

import codecs
from encodings import aliases

import pytest
from bs4 import BeautifulSoup

import bs4
from bs4.dammit import EntitySubstitution


def django_title(content: bytes):
    """The exact ``work_item_link_task.py:141-143`` expression."""
    soup = BeautifulSoup(content, "html.parser")
    tag = soup.find("title")
    return tag.get_text().strip() if tag else None


def test_oracle_library_versions():
    """The pins below were observed on these exact versions."""
    assert bs4.__version__ == "4.12.3"


def test_entity_probe_title():
    content = (
        b"<html><head><title>Docs &mdash; Getting started&nbsp;guide"
        b"</title></head></html>"
    )
    # NBSP (U+00A0) between "started" and "guide"; `.strip()` keeps it.
    assert django_title(content) == "Docs \u2014 Getting started\u00a0guide"


def test_windows1252_probe_title():
    content = (
        b'<html><head><meta charset="windows-1252">'
        b"<title>Caf\xe9 menu</title></head></html>"
    )
    assert django_title(content) == "Caf\u00e9 menu"


def test_shift_jis_probe_title():
    content = (
        b'<html><head><meta charset="shift_jis">'
        b"<title>\x93\xfa\x96\x7b\x8c\xea</title></head></html>"
    )
    assert django_title(content) == "\u65e5\u672c\u8a9e"


def test_declared_charset_beats_valid_utf8():
    """Meta wins even when the bytes are valid UTF-8 (C3 A9 → Ã©)."""
    content = (
        b'<html><head><meta charset="windows-1252">'
        b"<title>Caf\xc3\xa9</title></head></html>"
    )
    assert django_title(content) == "Caf\u00c3\u00a9"


def test_bom_beats_declared_charset():
    """BOM is tried before the meta declaration (C3 A9 → é)."""
    content = (
        b'\xef\xbb\xbf<html><head><meta charset="windows-1252">'
        b"<title>Caf\xc3\xa9</title></head></html>"
    )
    assert django_title(content) == "Caf\u00e9"


def test_failed_bom_decode_falls_through_to_declared():
    """BOM-sniffed UTF-8 fails strict on 0xE9; meta wins."""
    content = (
        b'\xef\xbb\xbf<html><head><meta charset="windows-1252">'
        b"<title>Caf\xe9</title></head></html>"
    )
    assert django_title(content) == "Caf\u00e9"


def test_charref_compensation_below_256():
    """`&#147;` numbers a Windows-1252 code point, not U+0093."""
    content = b"<html><head><title>&#147;</title></head></html>"
    assert django_title(content) == "\u201c"


def test_unknown_entity_drops_semicolon():
    assert django_title(b"<html><head><title>&unknown;</title></head></html>") == (
        "&unknown"
    )


@pytest.mark.parametrize(
    ("reference", "expected"),
    [
        ("&apos", "'"),
        ("&apos;", "'"),
        ("&notin", "\u2209"),
        ("&AMP", "&"),
        ("&LT", "<"),
    ],
)
def test_semicolonless_entities_convert(reference, expected):
    """BS4 substitutes semicolon-less names `html.unescape` leaves alone."""
    content = f"<html><head><title>{reference}</title></head></html>".encode("ascii")
    assert django_title(content) == expected


def test_entity_table_matches_bs4():
    """The generated Rust table's source: html5 minus `;`, first-wins.

    ``crates/services/src/tasks_webhooks/html_entities.rs`` is generated
    from CPython's ``html.entities.html5`` by exactly this rule; any
    drift (a bs4/Python upgrade reshaping the table) fails here first.
    """
    from html.entities import html5

    expect = {}
    for key in sorted(html5):
        bare = key[:-1] if key.endswith(";") else key
        if bare not in expect:
            expect[bare] = html5[key]
    got = EntitySubstitution.HTML_ENTITY_TO_CHARACTER
    assert len(expect) == 2125
    assert set(expect) == set(got)
    assert all(expect[key] == got[key] for key in expect)


def test_codec_alias_sets():
    """Exact alias sets the Rust label table mirrors.

    ``resolve_form`` in ``link_crawl.rs`` transcribes these spellings;
    a Python upgrade reshaping them must fail loudly here.
    """
    fams = {}
    for alias, canon in aliases.aliases.items():
        fams.setdefault(canon, set()).add(alias)
    assert fams["latin_1"] == {
        "8859",
        "cp819",
        "csisolatin1",
        "ibm819",
        "iso8859",
        "iso8859_1",
        "iso_8859_1",
        "iso_8859_1_1987",
        "iso_ir_100",
        "l1",
        "latin",
        "latin1",
    }
    assert fams["utf_16"] == {"u16", "utf16"}
    assert fams["utf_32"] == {"u32", "utf32"}
    assert fams["shift_jis"] == {
        "csshiftjis",
        "s_jis",
        "shiftjis",
        "sjis",
        "x_mac_japanese",
    }
    assert fams["mac_roman"] == {"macintosh", "macroman"}
    assert fams["ascii"] == {
        "646",
        "ansi_x3.4_1968",
        "ansi_x3.4_1986",
        "ansi_x3_4_1968",
        "cp367",
        "csascii",
        "ibm367",
        "iso646_us",
        "iso_646.irv_1991",
        "iso_ir_6",
        "us",
        "us_ascii",
    }
    for name in ("unicode", "x_sjis", "xsjis", "x-user-defined", "x_user_defined"):
        with pytest.raises(LookupError):
            codecs.lookup(name)
    assert codecs.lookup("windows_1252").name == "cp1252"
    assert codecs.lookup("u16").name == "utf-16"
