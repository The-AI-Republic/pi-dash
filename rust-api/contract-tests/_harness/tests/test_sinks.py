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
