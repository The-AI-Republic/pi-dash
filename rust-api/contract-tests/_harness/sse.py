"""Minimal SSE frame reader for contract suites (PIDASHCONV-98).

Parses ``text/event-stream`` framing per the SSE spec: ``event:`` / ``id:``
/ ``data:`` fields dispatched by a blank line, ``:`` lines as comments
(heartbeats). The chat event stream (``chat_event_stream``) emits frames of
the form::

    event: chat.event
    id: <seq>
    data: {...}

    : heartbeat

``collect_frames`` opens a streaming request on the caller's authenticated
httpx client, gathers frames until ``want_frames`` ``chat.event`` frames
arrive (or the deadline passes), then closes the stream — exercising the
server's close path.
"""

from __future__ import annotations

import time

EVENT_STREAM_CONTENT_TYPE = "text/event-stream"


def parse_sse_block(block: str) -> dict:
    """Parse one blank-line-delimited SSE block into a frame dict."""
    frame: dict = {"event": None, "id": None, "data": [], "comment": None}
    for line in block.split("\n"):
        if line == "":
            continue
        if line.startswith(":"):
            frame["comment"] = line[1:].strip()
        elif line.startswith("event:"):
            frame["event"] = line[len("event:"):].strip()
        elif line.startswith("id:"):
            frame["id"] = line[len("id:"):].strip()
        elif line.startswith("data:"):
            frame["data"].append(line[len("data:"):].strip())
    return frame


def collect_frames(client, method: str, url: str, *, want_frames: int = 1,
                   timeout_s: float = 25.0, **kwargs):
    """Stream SSE frames until ``want_frames`` event frames arrive.

    Returns ``(headers, frames)`` where ``frames`` holds every parsed block
    seen (events and comments) in order. Stops early once ``want_frames``
    blocks with an ``event`` field have arrived; otherwise stops at the
    deadline. Always closes the stream before returning.
    """
    deadline = time.monotonic() + timeout_s
    frames: list[dict] = []
    headers: dict = {}
    events_seen = 0
    with client.stream(method, url, timeout=timeout_s + 5.0, **kwargs) as response:
        headers = dict(response.headers)
        buffer = ""
        for line in response.iter_lines():
            buffer += line + "\n"
            if line == "":
                frame = parse_sse_block(buffer)
                buffer = ""
                frames.append(frame)
                if frame["event"] is not None:
                    events_seen += 1
                    if events_seen >= want_frames:
                        break
            if time.monotonic() >= deadline:
                break
    return headers, frames
