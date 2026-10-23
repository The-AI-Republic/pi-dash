"""PIDASHCONV-1410: link-crawler <title> RCDATA oracle pins.

Pins the Django-observed ``<title>`` texts the Rust crawler must
reproduce byte for byte. ``<title>`` is RCDATA (CPython
``html.parser`` lists ``title`` in ``RCDATA_CONTENT_ELEMENTS``,
``html/parser.py:116``): nested markup is literal text while entities
still decode, and the span ends only at ``</title`` followed by
``[\\t\\n\\r\\f />]`` (case-insensitive — the ``interesting`` pattern
``set_cdata_mode`` installs, ``html/parser.py:196-199``).

Each test replicates exactly
``apps/api/pi_dash/bgtasks/work_item_link_task.py:141-143``::

    soup = BeautifulSoup(response.content, "html.parser")
    title = title_tag.get_text().strip() if title_tag else None

with ``response.content`` as raw response bytes. No Django import, no
worker, no network: ``bs4`` is the only dependency beyond pytest.

Hermetic by necessity, not by preference (same reason as
``test_link_crawl_decoding.py``): the crawl task's SSRF guard
(``validate_url_ip``) rejects every loopback/private address, so no
live-crawl oracle can serve probe pages to the workers on an arbitrary
runner. The Rust side of this contract is pinned by unit tests in
``crates/services/src/tasks_webhooks/link_crawl.rs`` asserting the
same literals through ``crawl()`` with a stub transport (they fail on
pre-fix rust-dev).
"""

import bs4
from bs4 import BeautifulSoup


def django_title(content: bytes):
    """The exact ``work_item_link_task.py:141-143`` expression."""
    soup = BeautifulSoup(content, "html.parser")
    tag = soup.find("title")
    return tag.get_text().strip() if tag else None


def test_oracle_library_versions():
    """The pins below were observed on these exact versions."""
    assert bs4.__version__ == "4.12.3"


def test_nested_markup_is_literal_text():
    content = b"<title>A &amp; B <b>bold</b>&#33;</title>"
    assert django_title(content) == "A & B <b>bold</b>!"


def test_titlefoo_does_not_close():
    content = b"<title>a</titlefoo>b</title>"
    assert django_title(content) == "a</titlefoo>b"


def test_bare_lt_is_literal_text():
    content = b"<title>a < b</title>"
    assert django_title(content) == "a < b"


def test_comment_is_literal_text():
    content = b"<title>a<!--x-->b</title>"
    assert django_title(content) == "a<!--x-->b"


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


def test_close_needs_no_bracket():
    assert django_title(b"<title>a</title b rest") == "a"


def test_close_rejects_non_delimiters_and_eof():
    assert django_title(b"<title>a</title!>b</title>") == "a</title!>b"
    assert django_title(b"<title>a</title\x00>b</title>") == "a</title\x00>b"
    assert django_title(b"<title>a</ title>b</title>") == "a</ title>b"
    assert django_title(b"<title>a</title") == "a</title"


def test_unclosed_title_runs_to_end():
    assert django_title(b"<title>abc") == "abc"
    assert django_title(b"<title>a</titlefoo>") == "a</titlefoo>"
