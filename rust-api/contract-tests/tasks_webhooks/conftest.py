"""Fixtures for the webhooks task suite."""

import pytest

from _harness.sinks import ChunkedKeepAliveSink


@pytest.fixture(scope="session")
def chunked_keepalive_sink():
    """A 200 chunked sink that keeps the connection open past 30 s."""
    sink = ChunkedKeepAliveSink()
    sink.start()
    yield sink
    sink.stop()
