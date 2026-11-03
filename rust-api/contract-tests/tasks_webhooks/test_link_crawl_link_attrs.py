"""PIDASHCONV-1671: link-crawler <link> attribute oracle pins.

Pins the Django-observed favicon discovery the Rust crawler must
reproduce: ``<link>`` attribute values are HTML-unescaped, ``rel`` is
matched after whitespace normalisation, and the last of duplicate
attributes wins. Each test replicates exactly
``apps/api/pi_dash/bgtasks/work_item_link_task.py:194-199``::

    favicon_tag = soup.select_one(selector)
    if favicon_tag and favicon_tag.get("href"):
        favicon_href = urljoin(base_url, favicon_tag["href"])

with the four selectors at :187-192, over
``BeautifulSoup(content, "html.parser")``. The pins assert the
(selector, raw href) pair — ``urljoin`` is already ported and pinned
elsewhere; the Rust unit tests assert the joined discovery URL.

No Django import, no worker, no network: ``bs4`` is the only
dependency beyond pytest.

Hermetic by necessity, not by preference (same reason as
``test_link_crawl_decoding.py``): the crawl task's SSRF guard
(``validate_url_ip``) rejects every loopback/private address, so no
live-crawl oracle can serve probe pages to the workers on an arbitrary
runner. The Rust side of this contract is pinned by unit tests in
``crates/services/src/tasks_webhooks/link_crawl.rs`` asserting the
same literals through ``find_favicon_url`` with a stub transport (they
fail on pre-fix rust-dev); the bs4 pins below are the oracle those
tests reproduce.
"""

import bs4
from bs4 import BeautifulSoup


def django_find(content: bytes):
    """The exact ``work_item_link_task.py:187-199`` discovery expression."""
    soup = BeautifulSoup(content, "html.parser")
    for selector in (
        'link[rel="icon"]',
        'link[rel="shortcut icon"]',
        'link[rel="apple-touch-icon"]',
        'link[rel="apple-touch-icon-precomposed"]',
    ):
        tag = soup.select_one(selector)
        if tag and tag.get("href"):
            return (selector, tag["href"])
    return None


def test_oracle_library_versions():
    """The pins below were observed on these exact versions."""
    assert bs4.__version__ == "4.12.3"


def test_href_entity_is_unescaped():
    content = b'<link rel="icon" href="/f.ico?a=1&amp;b=2">'
    assert django_find(content) == ('link[rel="icon"]', "/f.ico?a=1&b=2")


def test_rel_surrounding_whitespace_matches():
    content = b'<link rel=" icon " href="a.ico">'
    assert django_find(content) == ('link[rel="icon"]', "a.ico")


def test_rel_inner_whitespace_collapses():
    content = b'<link rel="shortcut  icon" href="b.ico">'
    assert django_find(content) == ('link[rel="shortcut icon"]', "b.ico")


def test_last_duplicate_href_wins():
    content = b'<link rel="icon" href="1.ico" href="2.ico">'
    assert django_find(content) == ('link[rel="icon"]', "2.ico")


def test_rel_tab_and_newline_split():
    assert django_find(b'<link rel="shortcut\ticon" href="h.ico">') == (
        'link[rel="shortcut icon"]',
        "h.ico",
    )
    assert django_find(b'<link rel="shortcut\nicon" href="i.ico">') == (
        'link[rel="shortcut icon"]',
        "i.ico",
    )


def test_rel_extra_token_order_and_case_miss():
    # Not token containment: extras, reorder and case all miss.
    assert django_find(b'<link rel="icon x" href="c.ico">') is None
    assert django_find(b'<link rel="x icon" href="d.ico">') is None
    assert django_find(b'<link rel="ICON" href="e.ico">') is None
    assert (
        django_find(b'<link rel="shortcut icon extra" href="f.ico">') is None
    )
    assert django_find(b'<link rel="icon shortcut" href="g.ico">') is None
    assert django_find(b'<link rel="" href="j.ico">') is None
    assert django_find(b"<link>") is None


def test_rel_entity_decodes_before_match():
    assert django_find(b'<link rel="&#105;con" href="e1.ico">') == (
        'link[rel="icon"]',
        "e1.ico",
    )


def test_last_duplicate_rel_wins():
    assert django_find(b'<link rel="other" rel="icon" href="n.ico">') == (
        'link[rel="icon"]',
        "n.ico",
    )
    assert django_find(b'<link rel="icon" rel="other" href="o.ico">') is None


def test_attr_unescape_spots():
    """Attribute decoding is ``html.unescape``, not title decoding.

    ``&unknown;`` keeps its ``;`` and ``&apos`` stays literal here,
    while the ``<title>`` decoder drops the ``;`` and converts
    ``&apos`` — the two must not share an implementation.
    """
    cases = [
        ("&unknown;", "&unknown;"),
        ("&apos", "&apos"),
        ("&#65B", "AB"),
        ("&notin", "\u00acin"),
        ("&notin;", "\u2209"),
        ("&#147;", "\u201c"),
        ("&AMP", "&"),
        ("&lt;tag&gt;", "<tag>"),
    ]
    for raw, expected in cases:
        content = f'<link rel="icon" href="{raw}">'.encode("ascii")
        assert django_find(content) == ('link[rel="icon"]', expected), raw


def test_exact_entity_table_size():
    """Drift alarm for the shared exact table (PIDASHCONV-1553).

    ``pidash_types::html_entities::html_unescape`` looks keys up
    verbatim (``notin;`` in, bare ``notin`` out); a CPython upgrade
    reshaping ``html.entities.html5`` must fail loudly here so the
    generated table is refreshed with it.
    """
    from html.entities import html5

    assert len(html5) == 2231
    assert "notin;" in html5
    assert "notin" not in html5
