"""PIDASHCONV-1675: link-crawler validate-path normalization oracle pins.

Pins the Django-observed ``urllib.parse`` + ``requests`` results the Rust
crawler must reproduce for the three follow-up gaps behind PIDASHCONV-1668:

1. ``requests`` exposes response headers latin-1-decoded, so a UTF-8
   ``Location`` arrives as the mojibake ``str`` Django's redirect loop
   follows (``work_item_link_task.py:104-108``); non-UTF-8 bytes never get
   that far — ``requests.get(allow_redirects=False)`` still resolves the
   redirect target eagerly (``sessions.py:739`` →
   ``get_redirect_target`` → ``location.encode("latin1").decode("utf8")``)
   and raises ``UnicodeDecodeError`` there.
2. ``urlparse`` strips leading C0/space and drops ``\\t``/``\\r``/``\\n``
   before scheme detection, so direct ``validate_url_ip`` inputs like
   ``"  https://x.com/a"`` validate (``work_item_link_task.py:38-46``).
3. ``urlparse(...).hostname`` / ``f"{scheme}://{netloc}"`` derive from the
   parsed authority, so ``"https:foo://bar"`` has no hostname and the
   favicon fallback keeps the lower-cased scheme with the raw netloc
   (``work_item_link_task.py:202-203``).

Each ``urlparse`` assertion calls the exact stdlib function Django calls;
each ``Location`` row drives ``requests`` directly against a loopback
probe server (no SSRF guard involved, no routable address needed).

Hermetic by necessity, not by preference (same reason as
``test_link_crawl_urljoin.py``): the crawl task's SSRF guard rejects
every loopback/private address, so no live-crawl oracle can serve probe
pages to the workers on an arbitrary runner. The Rust side of this
contract is pinned by unit tests asserting the same literals through
``url_scheme()`` / ``url_hostname()`` / ``url_origin()`` /
``validate_url_ip()`` / ``crawl()`` in
``crates/services/src/tasks_webhooks/link_crawl.rs`` and through the
real ``reqwest`` adapter against a loopback 302 server in
``crates/jobs/src/tasks_webhooks/link_crawl.rs`` (they fail on pre-fix
rust-dev).
"""

import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urlparse

import pytest
import requests

# Fix 2: WHATWG control stripping before scheme detection. Every value
# was observed on CPython 3.12 (see the PIDASHCONV-1675 probe output in
# the workpad); the last four rows are unchanged-behavior controls.
SCHEME_ROWS = [
    ("  https://x.com/a", "https"),
    ("\x00\x01 https://x.com/a", "https"),
    ("\thttps://x.com/a", "https"),
    ("a\tb:c", "ab"),
    ("java\tscript:alert(1)", "javascript"),
    ("java\nscript:x", "javascript"),
    ("java\rscript:x", "javascript"),
    ("https://x.com/a", "https"),
    ("HTTP://x.com/a", "http"),
    ("1http://x.com/", ""),
    ("", ""),
]

# Fix 3: hostname comes from the parsed netloc (last-`@` userinfo split,
# first-`:` port split, `[`/`]` brackets, lower-cased host with the `%`
# zone id untouched), or is None when there is no authority.
HOSTNAME_ROWS = [
    ("http://user:pass@Example.COM:8080/x", "example.com"),
    ("http://a@b@c/", "c"),
    ("http://example.com:8080/", "example.com"),
    ("http://[::1]:8080/x", "::1"),
    ("http://[FE80::1%ETH0]/x", "fe80::1%ETH0"),
    ("http://M\xdcNCHEN.de/", "m\xfcnchen.de"),
    ("https:foo://bar", None),
    ("http:///no-host", None),
    ("file:///etc/passwd", None),
    ("  https://X.com/a", "x.com"),
    ("https://x.com/a", "x.com"),
]

# Fix 3: the favicon fallback is `f"{scheme}://{netloc}"` — lower-cased
# scheme, raw netloc — with no fallback origin when either half is empty.
ORIGIN_ROWS = [
    ("HTTPS://X.COM/A", "https://X.COM"),
    ("http://h:1/a?b#c", "http://h:1"),
    ("https://x.com/a", "https://x.com"),
    ("https:foo://bar", None),
    ("file:///etc/x", None),
    ("https:foo", None),
]


@pytest.mark.parametrize("url,expected", SCHEME_ROWS)
def test_scheme_sees_whatwg_normalized_input(url, expected):
    assert urlparse(url).scheme == expected


@pytest.mark.parametrize("url,expected", HOSTNAME_ROWS)
def test_hostname_comes_from_parsed_netloc(url, expected):
    assert urlparse(url).hostname == expected


@pytest.mark.parametrize("url,expected", ORIGIN_ROWS)
def test_origin_is_lowercased_scheme_plus_raw_netloc(url, expected):
    parsed = urlparse(url)
    origin = (
        f"{parsed.scheme}://{parsed.netloc}"
        if parsed.scheme and parsed.netloc
        else None
    )
    assert origin == expected


class _LocationProbe(BaseHTTPRequestHandler):
    # `send_header` frames values as latin-1, so the on-wire bytes are
    # exactly the latin-1 encoding of each literal below.

    def do_GET(self):  # noqa: N802
        if self.path == "/utf8":
            # Wire bytes `b"/caf\\xc3\\xa9"`: valid UTF-8, exposed by
            # requests as the latin-1 mojibake Django follows.
            self.send_response(302)
            self.send_header("Location", "/caf\xc3\xa9")
            self.end_headers()
        elif self.path == "/lone":
            # Wire byte `b"/caf\\xe9"`: not valid UTF-8, so
            # `get_redirect_target` raises while resolving `r._next`.
            self.send_response(302)
            self.send_header("Location", "/caf\xe9")
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


def test_utf8_location_is_followable_mojibake(location_probe_url):
    """UTF-8 ``Location`` bytes surface as truthy latin-1 text, so
    Django's ``if not redirect_url: break`` does not trigger and the
    mojibake URL is joined and fetched."""
    response = requests.get(
        f"{location_probe_url}/utf8", allow_redirects=False, timeout=5
    )
    assert response.status_code == 302
    assert response.is_redirect
    assert response.headers.get("Location") == "/caf\xc3\xa9"


def test_non_utf8_location_raises_before_django_sees_it(location_probe_url):
    """Non-UTF-8 ``Location`` bytes raise ``UnicodeDecodeError`` (a
    ``ValueError``) inside ``requests.get`` itself, so Django warns and
    never follows them."""
    with pytest.raises(UnicodeDecodeError):
        requests.get(
            f"{location_probe_url}/lone", allow_redirects=False, timeout=5
        )
