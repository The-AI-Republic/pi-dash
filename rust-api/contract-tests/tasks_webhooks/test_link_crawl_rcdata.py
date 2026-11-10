"""PIDASHCONV-1410: link-crawler <title> oracle pins.

Pins the Django-observed ``<title>`` texts the Rust crawler must
reproduce byte for byte. On the production interpreter (stock CPython
3.12.10 — ``apps/api/Dockerfile.api`` — and every stock release before
3.12.12) ``html.parser`` has no RCDATA support: ``title`` is a normal
element (``CDATA_CONTENT_ELEMENTS`` is just ``("script", "style")``),
so nested markup is parsed, comments become child nodes ``get_text``
skips, and unknown end tags are dropped while the title span continues
to ``</title`` (matched case-insensitively by ``endtagfind``, which
tolerates attributes, a blank before the name, and trailing space or
``/``). An unterminated ``</title`` at EOF is literal text.

Each test replicates exactly
``apps/api/pi_dash/bgtasks/work_item_link_task.py:141-143``::

    soup = BeautifulSoup(response.content, "html.parser")
    title = title_tag.get_text().strip() if title_tag else None

with ``response.content`` as raw response bytes. No Django import, no
worker, no network: ``bs4`` is the only dependency beyond pytest.

Interpreter requirement (NEWPIDASH-30): the pins below are the
production-parser behavior. Upstream added RCDATA title handling in
3.12.12 (and some distro builds backported it to earlier 3.12.x), on
which ``<title>`` keeps nested markup literally and several pins read
differently. Tests that differ across the two parsers skip on RCDATA
interpreters instead of asserting the wrong oracle; run them on stock
CPython 3.12.10, the version CI uses.

Hermetic by necessity, not by preference (same reason as
``test_link_crawl_decoding.py``): the crawl task's SSRF guard
(``validate_url_ip``) rejects every loopback/private address, so no
live-crawl oracle can serve probe pages to the workers on an arbitrary
runner. The Rust side of this contract is pinned by unit tests in
``crates/services/src/tasks_webhooks/link_crawl.rs`` asserting the
same literals through ``crawl()`` with a stub transport (they fail on
pre-fix rust-dev).
"""

from html.parser import HTMLParser

import bs4
import pytest
from bs4 import BeautifulSoup


def django_title(content: bytes):
    """The exact ``work_item_link_task.py:141-143`` expression."""
    soup = BeautifulSoup(content, "html.parser")
    tag = soup.find("title")
    return tag.get_text().strip() if tag else None


# See the module docstring: these pins are the production (pre-RCDATA)
# parser behavior and read differently where RCDATA was backported.
requires_legacy_parser = pytest.mark.skipif(
    hasattr(HTMLParser, "RCDATA_CONTENT_ELEMENTS"),
    reason="pins stock html.parser (<=3.12.11, production 3.12.10); "
    "this interpreter parses <title> as RCDATA (NEWPIDASH-30)",
)


def test_oracle_library_versions():
    """The pins below were observed on these exact versions."""
    assert bs4.__version__ == "4.12.3"


@requires_legacy_parser
def test_nested_markup_is_parsed():
    # No RCDATA: <b> is a child element, so get_text() drops the tags
    # while entities still decode.
    content = b"<title>A &amp; B <b>bold</b>&#33;</title>"
    assert django_title(content) == "A & B bold!"


@requires_legacy_parser
def test_titlefoo_does_not_close():
    # </titlefoo> is an end tag for an unknown element: dropped, and
    # the title span continues past it to </title>.
    content = b"<title>a</titlefoo>b</title>"
    assert django_title(content) == "ab"


def test_bare_lt_is_literal_text():
    content = b"<title>a < b</title>"
    assert django_title(content) == "a < b"


@requires_legacy_parser
def test_comment_inside_title_yields_text_only():
    # The comment is a child node; get_text() skips it.
    content = b"<title>a<!--x-->b</title>"
    assert django_title(content) == "ab"


def test_close_is_case_insensitive():
    assert django_title(b"<title>a</TITLE>b</title>") == "a"


def test_close_allows_trailing_space_slash_and_blank():
    assert django_title(b"<title>a</title >b</title>") == "a"
    assert django_title(b"<title>a</title/>b") == "a"
    assert django_title(b"<title>a</title\t>b") == "a"
    assert django_title(b"<title>a</title\n>b") == "a"
    assert django_title(b"<title>a</title\r>b") == "a"
    assert django_title(b"<title>a</title\x0c>b") == "a"
    assert django_title(b"<title>a</title foo=baz>rest") == "a"


@requires_legacy_parser
def test_unterminated_close_is_literal_text():
    # No ">" anywhere: parse_endtag never matches, so the would-be
    # close tag stays text through EOF.
    assert django_title(b"<title>a</title b rest") == "a</title b rest"


@requires_legacy_parser
def test_close_tag_punctuation_and_eof():
    # "</title!>": parsed as an end tag for the unknown element
    # "title!", which is dropped, so the text continues past it.
    assert django_title(b"<title>a</title!>b</title>") == "ab"
    # No NUL vector: a NUL byte flips UnicodeDammit to UTF-16 (via
    # charset-normalizer when installed) so Django returns None — a
    # charset-sniffing quirk (PIDASHCONV-1410 review).
    # A blank between "</" and the name still closes (endtagfind's
    # "</\\s*").
    assert django_title(b"<title>a</ title>b</title>") == "a"
    # EOF before ">": the partial close tag is literal text.
    assert django_title(b"<title>a</title") == "a</title"


@requires_legacy_parser
def test_unclosed_title_runs_to_end():
    assert django_title(b"<title>abc") == "abc"
    assert django_title(b"<title>a</titlefoo>") == "a"
