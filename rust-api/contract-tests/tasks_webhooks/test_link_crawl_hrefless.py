"""PIDASHCONV-1674: link-crawler hrefless/empty-href oracle pins.

Pins the Django-observed favicon discovery the Rust crawler must
reproduce: the FIRST ``<link>`` tag matching a selector wins that
selector, and a missing or empty ``href`` on it disqualifies the
selector — Django moves on to the NEXT SELECTOR, never to the next
tag. Each test replicates exactly
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


def test_hrefless_first_disqualifies_selector():
    content = b'<link rel="icon"><link rel="icon" href="/b.ico">'
    assert django_find(content) is None


def test_empty_href_first_disqualifies_selector():
    content = b'<link rel="icon" href=""><link rel="icon" href="/b.ico">'
    assert django_find(content) is None


def test_hrefless_and_empty_href_only_miss():
    assert django_find(b'<link rel="icon">') is None
    assert django_find(b'<link rel="icon" href="">') is None


def test_first_href_wins_over_later_same_selector_tag():
    # Control: with hrefs on both, the FIRST matching tag wins —
    # positional, not "any tag with an href".
    content = b'<link rel="icon" href="/a.ico"><link rel="icon" href="/b.ico">'
    assert django_find(content) == ('link[rel="icon"]', "/a.ico")


def test_next_selector_still_wins_after_hrefless():
    # Control: a disqualified selector falls through to the next one.
    content = b'<link rel="icon"><link rel="shortcut icon" href="/c.ico">'
    assert django_find(content) == ('link[rel="shortcut icon"]', "/c.ico")
    content = b'<link rel="icon" href=""><link rel="shortcut icon" href="/c.ico">'
    assert django_find(content) == ('link[rel="shortcut icon"]', "/c.ico")
