"""PIDASHCONV-1668: link-crawler URL-joining oracle pins.

Pins the Django-observed ``urllib.parse`` results the Rust crawler must
reproduce byte for byte: the five ``urljoin`` rows from the issue, the
RFC 3986 reference vectors, the ``urlparse().scheme`` rule behind
``validate_url_ip`` (``work_item_link_task.py:38-46``), and the
``requests``-level premise of ``if not redirect_url: break``
(``work_item_link_task.py:105-107``).

Each ``urljoin``/``urlparse`` assertion calls the exact stdlib function
Django calls at ``work_item_link_task.py:108`` / ``:197`` / ``:38`` — no
Django import, no worker, no network beyond a loopback probe server for
the ``Location`` rows (the SSRF guard is never invoked; ``requests``
is called directly, so no routable address is needed).

Hermetic by necessity, not by preference (same reason as
``test_link_crawl_decoding.py``): the crawl task's SSRF guard
(``validate_url_ip``) rejects every loopback/private address, so no
live-crawl oracle can serve probe pages to the workers on an arbitrary
runner. The Rust side of this contract is pinned by unit tests in
``crates/services/src/tasks_webhooks/link_crawl.rs`` asserting the
same literals through ``urljoin()`` / ``url_scheme()`` / ``safe_get()``
/ ``find_favicon_url()`` / ``crawl()`` with a stub transport (they fail
on pre-fix rust-dev); the full worker oracle was additionally verified
manually on both backends over a guard-routable address (see the
PIDASHCONV-1668 workpad).
"""

import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urljoin, urlparse

import pytest
import requests


# The five issue rows, in order.
ISSUE_ROWS = [
    (
        "https://x.com/a/b",
        "/login?next=https://x.com/foo",
        "https://x.com/login?next=https://x.com/foo",
    ),
    (
        "https://x.com/a/b",
        "data:image/png;base64,AAAA",
        "data:image/png;base64,AAAA",
    ),
    (
        "https://x.com/a/b?u=/p/q",
        "fav.ico",
        "https://x.com/a/fav.ico",
    ),
    (
        "https://x.com/a/b",
        "?page=2",
        "https://x.com/a/b?page=2",
    ),
    (
        "https://x.com/a/b/",
        "../f.ico",
        "https://x.com/a/f.ico",
    ),
]

# Guards for the full port: params, scheme-relative refs, fragments,
# empty refs, dot segments above the root, control characters, and the
# same-scheme-absolute reconstruction. Every value was observed on
# CPython 3.12 (see the PIDASHCONV-1668 probe output in the workpad).
PORT_ROWS = [
    ("https://x.com/a/b;p?q#f", "?x=1", "https://x.com/a/b;p?x=1"),
    ("https://x.com/a/b", "//other.com/p?q", "https://other.com/p?q"),
    ("https://x.com/a/b?q#f", "#frag2", "https://x.com/a/b?q#frag2"),
    ("https://x.com/a/b#f", "", "https://x.com/a/b#f"),
    ("", "rel/path", "rel/path"),
    ("https://x.com/a/b", "../../..", "https://x.com/"),
    ("https://x.com/a/b/c", "./d/./e", "https://x.com/a/b/d/e"),
    ("https://x.com/a/b", "HTTP://X.COM/A", "HTTP://X.COM/A"),
    ("http://x.com/a/b", "HTTP://X.COM/A", "http://X.COM/A"),
    ("https://x.com/a/b", "mailto:u@x.com", "mailto:u@x.com"),
    ("https://x.com/a/b", "java\tscript:alert(1)", "java\tscript:alert(1)"),
    ("https://x.com/a/b", "\n/path", "https://x.com/path"),
    ("https://x.com/a/b;p", "g;x", "https://x.com/a/g;x"),
    ("https://x.com", "?q", "https://x.com?q"),
    ("https://x.com", "a", "https://x.com/a"),
    ("file:///etc/passwd", "x", "file:///etc/x"),
    ("https://x.com/a/b", ".", "https://x.com/a/"),
    ("https://x.com/a/b", "..", "https://x.com/"),
    ("http://x.com/a/b", "//Y.COM/p", "http://Y.COM/p"),
    ("https://x.com/a/b?u=1#2", "p", "https://x.com/a/p"),
    ("https://x.com/a/b", "p?q#f", "https://x.com/a/p?q#f"),
    ("https://x.com/a/", ".", "https://x.com/a/"),
    ("https://x.com/a/b", "a/b/.", "https://x.com/a/a/b/"),
    ("  https://x.com/a", "b", "https://x.com/b"),
    ("https://x.com/a/b", "g:h", "g:h"),
    ("https://x.com/a/b", "?", "https://x.com/a/b"),
    ("https://x.com/a/b", "#", "https://x.com/a/b"),
    ("https://x.com/a/b", "/", "https://x.com/"),
    ("https://x.com/a/b/c/", "../../g", "https://x.com/a/g"),
]

# RFC 3986 section 5.4 reference vectors (base has params + query).
RFC_BASE = "http://x.com/a/b/c/d;p?q"
RFC_ROWS = [
    ("g", "http://x.com/a/b/c/g"),
    ("./g", "http://x.com/a/b/c/g"),
    ("g/", "http://x.com/a/b/c/g/"),
    ("/g", "http://x.com/g"),
    ("//y.com/g", "http://y.com/g"),
    ("?y", "http://x.com/a/b/c/d;p?y"),
    ("g?y", "http://x.com/a/b/c/g?y"),
    ("#s", "http://x.com/a/b/c/d;p?q#s"),
    ("g#s", "http://x.com/a/b/c/g#s"),
    ("g?y#s", "http://x.com/a/b/c/g?y#s"),
    (";x", "http://x.com/a/b/c/;x"),
    ("g;x", "http://x.com/a/b/c/g;x"),
    ("g;x?y#s", "http://x.com/a/b/c/g;x?y#s"),
    ("", "http://x.com/a/b/c/d;p?q"),
    (".", "http://x.com/a/b/c/"),
    ("./", "http://x.com/a/b/c/"),
    ("..", "http://x.com/a/b/"),
    ("../", "http://x.com/a/b/"),
    ("../g", "http://x.com/a/b/g"),
    ("../..", "http://x.com/a/"),
    ("../../", "http://x.com/a/"),
    ("../../g", "http://x.com/a/g"),
]

SCHEME_ROWS = [
    ("/login?next=https://x.com/foo", ""),
    ("data:image/png;base64,AAAA", "data"),
    ("HTTP://x.com/a", "http"),
    ("1http://x.com/", ""),
    ("a+b.c-d://x.com/", "a+b.c-d"),
    ("://x.com/", ""),
    ("https://x.com/a", "https"),
    ("file:///etc/passwd", "file"),
    ("mailto:u@x.com", "mailto"),
    ("tel:+1-816-555-1212", "tel"),
    ("a:b/c", "a"),
    ("a/b:c", ""),
    ("", ""),
]


@pytest.mark.parametrize("base,rel,expected", ISSUE_ROWS)
def test_issue_urljoin_row(base, rel, expected):
    assert urljoin(base, rel) == expected


@pytest.mark.parametrize("base,rel,expected", PORT_ROWS)
def test_port_guard_row(base, rel, expected):
    assert urljoin(base, rel) == expected


@pytest.mark.parametrize("rel,expected", RFC_ROWS)
def test_rfc3986_reference_vector(rel, expected):
    assert urljoin(RFC_BASE, rel) == expected


@pytest.mark.parametrize("url,expected", SCHEME_ROWS)
def test_scheme_comes_only_from_valid_leading_scheme(url, expected):
    assert urlparse(url).scheme == expected


class _LocationProbe(BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802
        if self.path == "/empty":
            self.send_response(302)
            self.send_header("Location", "")
            self.end_headers()
        elif self.path == "/missing":
            self.send_response(302)
            self.end_headers()
        else:  # pragma: no cover
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok")

    def log_message(self, *args):
        pass


@pytest.fixture(scope="module")
def location_probe_url():
    server = HTTPServer(("127.0.0.1", 0), _LocationProbe)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{server.server_port}"
    server.shutdown()
    thread.join()


def test_empty_location_enters_redirect_loop_but_breaks(location_probe_url):
    """A present-but-empty ``Location`` keeps ``is_redirect`` true while
    ``headers.get("Location")`` is falsy — so Django's
    ``if not redirect_url: break`` returns the redirect response as-is
    without another fetch."""
    response = requests.get(
        f"{location_probe_url}/empty", allow_redirects=False, timeout=5
    )
    assert response.status_code == 302
    assert response.is_redirect
    assert not response.headers.get("Location")


def test_missing_location_never_enters_redirect_loop(location_probe_url):
    response = requests.get(
        f"{location_probe_url}/missing", allow_redirects=False, timeout=5
    )
    assert response.status_code == 302
    assert not response.is_redirect
    assert response.headers.get("Location") is None
