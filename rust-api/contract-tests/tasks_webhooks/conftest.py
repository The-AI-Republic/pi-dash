"""Fixtures for the webhooks task suite."""

import pytest

from _harness.sinks import ChunkedKeepAliveSink, ChunkedStreamSink


@pytest.fixture(scope="session")
def chunked_keepalive_sink():
    """A 200 chunked sink that keeps the connection open past 30 s."""
    sink = ChunkedKeepAliveSink()
    sink.start()
    yield sink
    sink.stop()


@pytest.fixture(scope="session")
def chunked_stream_sink():
    """A 200 chunked sink streaming 64 MiB in 8k paced 8 KiB chunks."""
    sink = ChunkedStreamSink()
    sink.start()
    yield sink
    sink.stop()
