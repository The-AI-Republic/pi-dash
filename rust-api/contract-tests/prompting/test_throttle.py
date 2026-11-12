"""UserRateThrottle on every prompting view (PIDASHCONV-1229).

Django declares ``throttle_classes = [UserRateThrottle]`` on all four
prompting views (``prompting/views.py:153,184,326,364``): scope ``user``,
120/minute (``settings/common.py:96``), one shared cache key
``throttle_user_<user pk>``. DRF checks it in ``APIView.initial()`` after
session auth/CSRF and before the view body, so every prompting handler
answers the rewritten 429 once the user has spent 120 requests in 60 s.
"""

import os

import pytest

from _harness.throttle import reset_throttle_key

RATE_LIMIT_BODY = '{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}'


def _user_throttle_key(user_id):
    # SimpleRateThrottle.cache_format = "throttle_%(scope)s_%(ident)s".
    return f"throttle_user_{user_id}"


def _assert_throttled(response):
    assert response.status_code == 429, (response.status_code, response.text)
    assert response.text == RATE_LIMIT_BODY, response.text
    assert response.headers["content-type"] == "application/json"


def test_user_throttle_is_shared_by_every_prompting_handler(
    api, tenant_a, auth_a, write_a
):
    if not os.environ.get("REDIS_URL"):
        pytest.fail("prompting throttle test requires REDIS_URL to reset the counter")
    key = _user_throttle_key(tenant_a["user"]["id"])
    reset_throttle_key(key)
    try:
        slug = tenant_a["workspace"]["slug"]
        list_url = f"/api/workspaces/{slug}/prompt-sections?kind=coding-task"
        detail_url = f"/api/workspaces/{slug}/prompt-sections/implementation"
        compiled_url = f"/api/workspaces/{slug}/prompts/coding-task/compiled"
        preview_url = f"/api/workspaces/{slug}/prompts/coding-task/preview"

        for attempt in range(120):
            r = api.get(list_url, headers=auth_a)
            assert r.status_code == 200, (attempt, r.status_code, r.text)

        # 121st request: DRF trims nothing, denies, and the project's
        # exception handler rewrites the body.
        _assert_throttled(api.get(list_url, headers=auth_a))

        # One shared key: all other prompting handlers are throttled too.
        _assert_throttled(api.get(compiled_url, headers=auth_a))
        _assert_throttled(
            api.put(detail_url, headers=write_a, json={"body": "throttled"})
        )
        _assert_throttled(api.delete(detail_url, headers=write_a))
        _assert_throttled(
            api.post(preview_url, headers=write_a, json={"scope": "workspace"})
        )
    finally:
        reset_throttle_key(key)
