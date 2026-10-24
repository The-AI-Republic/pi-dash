"""Loopback tests for the sink machinery. Needs no backend."""

import smtplib

import httpx

from _harness import sinks


def test_smtp_sink_captures_mail():
    with sinks.SmtpSink() as sink:
        with smtplib.SMTP(sink.host, sink.port) as client:
            client.sendmail("a@example.com", ["b@example.com"], "Subject: hi\r\n\r\nbody")
    assert len(sink.messages) == 1
    msg = sink.messages[0]
    assert msg["mail_from"] == "a@example.com"
    assert msg["rcpt_tos"] == ["b@example.com"]
    assert "body" in msg["data"]


def test_webhook_sink_captures_post():
    with sinks.WebhookSink() as sink:
        r = httpx.post(sink.url + "/hook", json={"event": "ping"}, timeout=10)
        assert r.status_code == 200
    assert len(sink.requests) == 1
    req = sink.requests[0]
    assert req["method"] == "POST"
    assert req["path"] == "/hook"
    assert req["json"] == {"event": "ping"}


def test_hostile_length_sink_answers_huge_length_then_closes():
    """The hostile sink emits the exact framing PIDASHCONV-1273 pins."""
    import socket

    with sinks.HostileLengthSink() as sink:
        with socket.create_connection((sink.host, sink.port), timeout=10) as conn:
            conn.sendall(
                b"POST /hook/x HTTP/1.1\r\nHost: t\r\nContent-Length: 2\r\n\r\n{}"
            )
            head = b""
            while b"\r\n\r\n" not in head:
                chunk = conn.recv(4096)
                assert chunk, "sink closed before answering"
                head += chunk
            assert head == (
                b"HTTP/1.1 200 OK\r\n"
                b"Content-Length: 1000000000000\r\n"
                b"Connection: close\r\n\r\n"
            )
            assert conn.recv(4096) == b"", "sink must close without a body"
    assert sink.connection_count() == 1
    assert b"POST /hook/x" in sink.requests[0]
