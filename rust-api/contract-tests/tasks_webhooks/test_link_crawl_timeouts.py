"""PIDASHCONV-1669: link-crawler timeout oracle pins.

Pins the ``requests`` timeout semantics the Rust crawler must
reproduce: ``timeout=N`` applies to the connect and to EACH socket
read, with no total deadline. ``requests.get(..., timeout=1)`` reads a
1.2s drip whole (no single gap reaches 1s) but raises on a 1.5s stall;
``requests.head(..., timeout=2)`` tolerates a 1.3s stall.

Hermetic by necessity, not by preference (same reason as
``test_link_crawl_decoding.py``): the crawl task's SSRF guard
(``validate_url_ip``) rejects every loopback/private address, so no
live-crawl oracle can serve probe pages to the workers on an arbitrary
runner. The Rust side of this contract is pinned by unit tests in
``crates/jobs/src/tasks_webhooks/link_crawl.rs`` asserting the same
shapes through the real HTTP adapter (the drip test fails on pre-fix
rust-dev); the full worker oracle was additionally verified manually
on both backends (see the PIDASHCONV-1669 workpad).
"""

import time

import pytest
import requests

from _harness.sinks import SlowDripSink


def test_oracle_library_line():
    """The pins below are requests 2.x connect/per-read semantics."""
    assert requests.__version__.split(".")[0] == "2"


def test_get_reads_drip_past_total_time():
    """A 0.6s + 0.6s drip reads whole under timeout=1 (no total deadline)."""
    with SlowDripSink(first_gap=0.6, second_gap=0.6) as sink:
        start = time.monotonic()
        response = requests.get(
            f"{sink.url}/slow", timeout=1, allow_redirects=False
        )
        elapsed = time.monotonic() - start
    assert response.status_code == 200
    assert response.content == SlowDripSink.BODY
    assert b"<title>Slow drip oracle</title>" in response.content
    assert elapsed >= 1.15, f"sink did not actually drip: {elapsed:.2f}s"


def test_get_times_out_on_stall():
    """A single 1.5s gap exceeds the 1s per-read budget.

    The headers arrive inside budget, so the stall fires while
    streaming the body — where requests surfaces the urllib3
    ``ReadTimeoutError`` as ``ConnectionError`` ("Read timed out"),
    not ``ReadTimeout``. Either way it is a ``RequestException`` and
    the crawl falls back exactly as for a header stall.
    """
    with SlowDripSink(first_gap=0.0, second_gap=1.5) as sink:
        with pytest.raises(
            requests.exceptions.RequestException, match="Read timed out"
        ):
            requests.get(f"{sink.url}/slow", timeout=1, allow_redirects=False)


def test_head_tolerates_two_second_reads():
    """The favicon HEAD budget is 2s (:208): a 1.3s stall still answers."""
    with SlowDripSink(first_gap=1.3, second_gap=0.0) as sink:
        response = requests.head(
            f"{sink.url}/slow", timeout=2, allow_redirects=False
        )
    assert response.status_code == 200
    assert response.content == b""
