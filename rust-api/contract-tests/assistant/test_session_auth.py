# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Assistant routes verify the full Django session, not just the user id.

Django's ``get_user`` requires ``_auth_user_backend`` to name a configured
backend and ``_auth_user_hash`` to match ``user.get_session_auth_hash()``;
a mismatch flushes the session, so every assistant endpoint answers as an
anonymous caller. These cases rewrite a live session row after a real
login and pin that behavior for the thread list, message POST and SSE
event stream.
"""

import pytest

from _harness.client import SESSION_COOKIE, rewrite_session

from .conftest import threads_url

UNAUTHENTICATED = {"detail": "Authentication credentials were not provided."}


def _new_thread(member, base):
    res = member.post(f"{base}/threads/", json={"title": "session"})
    assert res.status_code == 201
    return res.json()["id"]


@pytest.mark.parametrize("tamper", ["hash", "backend"])
def test_tampered_session_is_anonymous(world, member, tamper):
    base = threads_url(world)
    thread_id = _new_thread(member, base)
    key = member.cookies.get(SESSION_COOKIE)
    assert key

    if tamper == "hash":
        rewrite_session(key, world.member, auth_hash="0" * 64)
    else:
        rewrite_session(
            key,
            world.member,
            backend="django.contrib.auth.backends.RemoteUserBackend",
        )

    listed = member.get(f"{base}/threads/")
    assert listed.status_code == 401
    assert listed.json() == UNAUTHENTICATED

    posted = member.post(
        f"{base}/threads/{thread_id}/messages/", json={"content": "still me?"}
    )
    assert posted.status_code == 401
    assert posted.json() == UNAUTHENTICATED

    deleted = member.delete(f"{base}/threads/{thread_id}/")
    assert deleted.status_code == 401
    assert deleted.json() == UNAUTHENTICATED

    events = member.get(f"{base}/threads/{thread_id}/events/")
    assert events.status_code == 401
