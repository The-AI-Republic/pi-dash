"""Unit tests for the harness 429 retry (no server needed).

``ContractClient`` rides out the stock `anon` throttle so a full-file run
goes green against stock settings. These tests pin the retry behavior
itself with a mock transport: real throttle semantics are still pinned by
the live contract suites, which never assert a 429 shape.
"""

import httpx
import pytest

from _harness.http import ContractClient, _MAX_429_ATTEMPTS


def _client(statuses, *, retry_after=None):
    calls = {"n": 0}

    def handler(request):
        calls["n"] += 1
        status = statuses[min(calls["n"] - 1, len(statuses) - 1)]
        headers = {}
        if status == 429 and retry_after is not None:
            headers["retry-after"] = retry_after
        return httpx.Response(status, headers=headers)

    return ContractClient(transport=httpx.MockTransport(handler)), calls


def test_no_retry_without_429(monkeypatch):
    sleeps = []
    monkeypatch.setattr("time.sleep", sleeps.append)
    client, calls = _client([200])
    assert client.get("http://test/").status_code == 200
    assert calls["n"] == 1
    assert sleeps == []


def test_retries_429_then_returns_recovery(monkeypatch):
    sleeps = []
    monkeypatch.setattr("time.sleep", sleeps.append)
    client, calls = _client([429, 429, 200], retry_after="0")
    assert client.get("http://test/").status_code == 200
    assert calls["n"] == 3
    assert sleeps == [1.0, 1.0]


def test_gives_up_after_bounded_attempts(monkeypatch):
    sleeps = []
    monkeypatch.setattr("time.sleep", sleeps.append)
    client, calls = _client([429], retry_after="0")
    assert client.get("http://test/").status_code == 429
    assert calls["n"] == _MAX_429_ATTEMPTS
    assert len(sleeps) == _MAX_429_ATTEMPTS - 1
