"""PIDASHCONV-1670: link-crawler ignores tags inside comments and scripts.

Pins the Django-observed ``<title>`` texts and favicon ``href`` values
the Rust crawler must reproduce byte for byte. BeautifulSoup with
``html.parser`` treats ``<!--...-->`` comments as declarations and
``<script>`` / ``<style>`` contents as CDATA (``html/parser.py``:
``commentclose`` plus the ``</name(?=[\\t\\n\\r\\f />])`` interesting
pattern ``set_cdata_mode`` installs), so tags inside those regions are
never elements: the first real ``<title>`` and matching ``<link>`` win.

Each title test replicates exactly
``apps/api/pi_dash/bgtasks/work_item_link_task.py:141-143``::

    soup = BeautifulSoup(response.content, "html.parser")
    title = title_tag.get_text().strip() if title_tag else None

and each link test replicates ``:187-199`` (selection only; the network
fetch and ``validate_url_ip`` are out of scope for parsing pins)::

    for selector in favicon_selectors:
        favicon_tag = soup.select_one(selector)
        if favicon_tag and favicon_tag.get("href"):
            favicon_href = favicon_tag["href"]

with ``response.content`` as raw response bytes. No Django import, no
worker, no network: ``bs4`` is the only dependency beyond pytest.

Hermetic by necessity, not by preference (same reason as
``test_link_crawl_rcdata.py``): the crawl task's SSRF guard
(``validate_url_ip``) rejects every loopback/private address, so no
live-crawl oracle can serve probe pages to the workers on an arbitrary
runner. The Rust side of this contract is pinned by unit tests in
``crates/services/src/tasks_webhooks/link_crawl.rs`` asserting the
same literals through ``crawl()`` / ``find_favicon_url()`` with stub
transports (they fail on pre-fix rust-dev).
"""

import bs4
from bs4 import BeautifulSoup

FAVICON_SELECTORS = [
    'link[rel="icon"]',
    'link[rel="shortcut icon"]',
    'link[rel="apple-touch-icon"]',
    'link[rel="apple-touch-icon-precomposed"]',
]


def django_title(content: bytes):
    """The exact ``work_item_link_task.py:141-143`` expression."""
    soup = BeautifulSoup(content, "html.parser")
    tag = soup.find("title")
    return tag.get_text().strip() if tag else None


def django_href(content: bytes):
    """The exact ``work_item_link_task.py:194-199`` selection."""
    soup = BeautifulSoup(content, "html.parser")
    for selector in FAVICON_SELECTORS:
        tag = soup.select_one(selector)
        if tag and tag.get("href"):
            return tag["href"]
    return None


def test_oracle_library_versions():
    """The pins below were observed on these exact versions."""
    assert bs4.__version__ == "4.12.3"


def test_commented_title_is_not_an_element():
    content = b"<!-- <title>old</title> --><title>real</title>"
    assert django_title(content) == "real"


def test_script_title_is_not_an_element():
    content = b'<script>var a="<title>js</title>";</script><title>real</title>'
    assert django_title(content) == "real"


def test_commented_link_is_not_an_element():
    content = (
        b'<!-- <link rel="icon" href="old.ico"> -->'
        b'<link rel="icon" href="new.ico">'
    )
    assert django_href(content) == "new.ico"


def test_script_link_is_not_an_element():
    content = (
        b'<script>var a=\'<link rel="icon" href="js.ico">\';</script>'
        b'<link rel="icon" href="new.ico">'
    )
    assert django_href(content) == "new.ico"


def test_style_title_is_not_an_element():
    content = (
        b"<style>.x{content:'<title>css</title>'}</style><title>real</title>"
    )
    assert django_title(content) == "real"


def test_style_link_is_not_an_element():
    content = (
        b"<style>"
        b".x::before{content:\"<link rel='icon' href='css.ico'>\"}"
        b"</style>"
        b'<link rel="icon" href="new.ico">'
    )
    assert django_href(content) == "new.ico"


def test_element_matching_is_case_insensitive():
    assert (
        django_title(b"<SCRIPT>var a=1;</SCRIPT><title>real</title>") == "real"
    )
    assert (
        django_href(
            b"<STYLE>.x{}</STYLE>" b'<link rel="icon" href="new.ico">'
        )
        == "new.ico"
    )


def test_unclosed_comment_swallows_the_title():
    assert django_title(b"<!-- foo <title>real</title>") is None


def test_self_closed_script_opens_no_span():
    assert django_title(b"<script/><title>real</title>") == "real"


def test_script_close_allows_attributes():
    assert (
        django_title(b"<script>x</script foo=bar><title>real</title>")
        == "real"
    )


def test_first_script_close_wins_inside_strings():
    assert (
        django_title(b'<script>var a="</script>";</script><title>real</title>')
        == "real"
    )
