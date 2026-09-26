# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""SSE event stream: replay, cursors, auth and headers.

Covers ``GET threads/<id>/events/``: the ``text/event-stream`` content type
with buffering disabled, replay of persisted events (each ``data:`` frame
carries the serialised event shape), the ``after`` replay cursor, and the
auth ladder (anonymous 401, guest 404, cross-user thread 404). The live tail
hangs on a Redis subscription, so every case reads only the replay prefix
and closes the stream.
"""

import json
import time
import uuid

import httpx

from _harness.config import base_url
from _harness.db import db_cursor

from .conftest import threads_url


def _seed_event(thread_id, seq, kind="turn_started", payload=None):
    with db_cursor() as cur:
        cur.execute(
            """INSERT INTO assistant_event (thread_id, seq, kind, payload, created_at)
               VALUES (%s,%s,%s,%s::jsonb,now()) RETURNING id""",
            (thread_id, seq, kind, json.dumps(payload or {"x": 1})),
        )
        return cur.fetchone()[0]


def _new_thread(client, base):
    return client.post(f"{base}/threads/", json={"title": "s"}).json()["id"]


def _read_frames(client, url, *, params=None, max_frames=4, deadline_s=20.0):
    """Read SSE frames until ``max_frames`` data frames arrive, then close.

    The live tail never ends on its own (keepalives flow forever), so the
    read is bounded by a deadline: an empty replay fails the test instead
    of hanging the suite.
    """
    frames = []
    headers = {}
    end = time.monotonic() + deadline_s
    with client.stream("GET", url, params=params, timeout=15.0) as resp:
        headers = dict(resp.headers)
        status = resp.status_code
        buf = ""
        for chunk in resp.iter_text():
            buf += chunk
            while "\n\n" in buf:
                raw, buf = buf.split("\n\n", 1)
                if raw.startswith(":"):
                    continue
                frames.append(raw)
                if len(frames) >= max_frames:
                    break
            if len(frames) >= max_frames or time.monotonic() > end:
                break
    return status, headers, frames


def _data_payload(frame):
    assert frame.startswith("event: chat.event\n")
    data = [ln for ln in frame.splitlines() if ln.startswith("data: ")][0]
    return json.loads(data[len("data: "):])


def test_sse_replays_events_with_headers(world, member):
    base = threads_url(world)
    tid = _new_thread(member, base)
    _seed_event(tid, 1)
    _seed_event(tid, 2, kind="assistant_delta")

    status, headers, frames = _read_frames(member, f"{base}/threads/{tid}/events/")
    assert status == 200
    assert headers["content-type"] == "text/event-stream"
    assert headers["cache-control"] == "no-cache"
    assert headers["x-accel-buffering"] == "no"

    payloads = [_data_payload(f) for f in frames[:2]]
    assert [p["seq"] for p in payloads] == [1, 2]
    for p in payloads:
        assert set(p.keys()) >= {
            "id",
            "thread",
            "seq",
            "kind",
            "payload",
            "created_at",
        }
        assert p["thread"] == tid


def test_sse_after_cursor_skips_older_events(world, member):
    base = threads_url(world)
    tid = _new_thread(member, base)
    _seed_event(tid, 1)
    _seed_event(tid, 2)

    status, _, frames = _read_frames(
        member, f"{base}/threads/{tid}/events/", params={"after": 1}, max_frames=2
    )
    assert status == 200
    assert _data_payload(frames[0])["seq"] == 2


def test_sse_rejects_anonymous(world, anon):
    status, _, _ = _read_frames(
        anon, f"{threads_url(world)}/threads/{uuid.uuid4()}/events/", max_frames=1
    )
    assert status == 401


def test_sse_denies_guest_and_cross_user_threads(world, guest, member, admin):
    base = threads_url(world)
    tid = _new_thread(admin, base)

    status, _, _ = _read_frames(guest, f"{base}/threads/{tid}/events/", max_frames=1)
    assert status == 404

    status, _, _ = _read_frames(member, f"{base}/threads/{tid}/events/", max_frames=1)
    assert status == 404


def test_sse_missing_thread_is_404(world, member):
    status, _, _ = _read_frames(
        member, f"{threads_url(world)}/threads/{uuid.uuid4()}/events/", max_frames=1
    )
    assert status == 404


def test_sse_bad_after_falls_back_to_zero(world, member):
    base = threads_url(world)
    tid = _new_thread(member, base)
    _seed_event(tid, 1)
    # A raw client without a session cookie jar: proves the cursor parse path
    # without depending on httpx cookie behaviour.
    raw = httpx.Client(base_url=base_url(), timeout=15.0, cookies=member.cookies)
    status, _, frames = _read_frames(
        raw, f"{base}/threads/{tid}/events/", params={"after": "abc"}, max_frames=1
    )
    assert status == 200
    assert _data_payload(frames[0])["seq"] == 1
