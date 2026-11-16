# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: AI assistant write flows (PIDASHCONV-823).

Built from the ``assistant`` contract suite: test_llm_config.py (BYOK
lifecycle, validation, SSRF guard, missing-config probe,
generate-title gates), test_stt_transcribe.py (STT lifecycle,
transcribe gates), test_mcp_servers.py (CRUD, write-only credential,
duplicates, per-user 404s, SSRF guard), test_threads.py (create,
patch/delete, isolation, list reaping), test_messages.py (202 queued
turn + auto-title, turn_active brake, validation, cancel) and
test_permissions.py (desktop-only agent token, cross-workspace 404s).

Deliberately not replayed: the MCP 10-server cap loop (same route as
the covered creates, at 11x the requests), the configured /test/
probes (they dial a live provider), the 27 MB 413 upload (same route
as the covered transcribes), the SSE event stream (not a write) and
the task-oracle worker tests (not HTTP).

The seed plants four threads: a stale empty one the list reaps, a
fresh empty one and a stale titled one the list keeps, and a
loop-kind one the list hides — so the thread-list GET verifies the
reaping/filtering writes on both sides.
"""

from ..session import Request, Session

_PUBLIC_BASE = "https://8.8.8.8/v1"
_MCP_URL = "https://8.8.8.8/mcp"
_MISSING = "00000000-0000-0000-0000-000000000000"

_LLM_PUT = {
    "provider_kind": "openai_compatible",
    "base_url": _PUBLIC_BASE,
    "model_name": "m",
    "api_key": "test-key-123",
}
_STT_PUT = {
    "base_url": _PUBLIC_BASE,
    "model_name": "whisper-1",
    "api_key": "test-key-123",
}
_AUDIO = ("clip.webm", b"RIFFxxxxWAVE", "audio/webm")


def _seed_extras(conn) -> None:
    """Stale/fresh/titled/loop threads for the list-reaping GET."""
    with conn.cursor() as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = cur.fetchone()[0]
        cur.execute("SELECT id FROM users WHERE email = 'replay-member@example.com'")
        member_id = cur.fetchone()[0]
        rows = [
            ("", "chat", "now() - interval '2 hours'"),  # reaped
            ("", "chat", "now()"),  # kept: fresh
            ("Kept", "chat", "now() - interval '2 hours'"),  # kept: titled
            ("loop run", "loop", "now()"),  # hidden from the list
        ]
        for title, kind, created in rows:
            cur.execute(
                "INSERT INTO assistant_thread (id, workspace_id, user_id,"
                " title, kind, is_archived, created_at, updated_at)"
                f" VALUES (gen_random_uuid(), %s, %s, %s, %s, false, {created}, {created})",
                (ws_id, member_id, title, kind),
            )
    conn.commit()


_BASE = "/api/workspaces/{ws}/ai-assistant"
_OTHER_BASE = "/api/workspaces/{other_ws}/ai-assistant"
_THREADS = _BASE + "/threads"

SESSIONS = (
    Session(
        name="assistant_lifecycle",
        domain="assistant",
        source="assistant/test_llm_config.py, test_stt_transcribe.py, "
        "test_mcp_servers.py, test_threads.py, test_messages.py, "
        "test_permissions.py",
        auth_users=("admin", "member", "guest", "outsider"),
        seed=_seed_extras,
        # BYOK secrets at rest (assistant/crypto.py FernetBackend over
        # ASSISTANT_ENCRYPTION_KEY): random IV per encryption, so the
        # columns compare by decrypted plaintext.
        encrypted_columns={
            "assistant_user_llm_config": [("api_key_encrypted", "assistant")],
            "assistant_user_stt_config": [("api_key_encrypted", "assistant")],
            "assistant_mcp_server": [("auth_header_encrypted", "assistant")],
        },
        requests=(
            # -- LLM config: missing-config gates first (no config yet) --
            Request(
                "POST", _BASE + "/generate-title/",
                route="/api/workspaces/{}/ai-assistant/generate-title/",
                json={"description": "Ship the new dashboard export."},
                auth="member", expect_status=422,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/config/test/",
                route="/api/users/me/ai-assistant/config/test/",
                auth="member", expect_status=200,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/config/test/",
                route="/api/users/me/ai-assistant/config/test/",
                auth="anonymous", expect_status=401,
            ),
            # -- LLM config lifecycle + validation --
            Request(
                "PUT", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                json=dict(_LLM_PUT),
                auth="member", expect_status=200,
            ),
            Request(
                "GET", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                auth="member", expect_status=200,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                json={"model_name": "m2"},
                auth="member", expect_status=200,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                json=dict(_LLM_PUT, base_url="ftp://x/v1"),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                json=dict(_LLM_PUT, base_url="https://u:p@8.8.8.8/v1"),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                json=dict(_LLM_PUT, base_url=""),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                json=dict(_LLM_PUT, model_name=""),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                json=dict(_LLM_PUT, api_key="short"),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                json=dict(_LLM_PUT, base_url="http://169.254.169.254/v1"),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                json=dict(_LLM_PUT),
                auth="admin", expect_status=200,
            ),
            # -- generate-title with config set: blank description, guest --
            Request(
                "POST", _BASE + "/generate-title/",
                route="/api/workspaces/{}/ai-assistant/generate-title/",
                json={"description": "   "},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", _BASE + "/generate-title/",
                route="/api/workspaces/{}/ai-assistant/generate-title/",
                json={"description": "Ship it."},
                auth="guest", expect_status=403,
            ),
            # -- STT config: missing-config gates first --
            Request(
                "POST", "/api/users/me/ai-assistant/stt-config/test/",
                route="/api/users/me/ai-assistant/stt-config/test/",
                auth="member", expect_status=200,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/transcribe/",
                route="/api/users/me/ai-assistant/transcribe/",
                files={"file": _AUDIO},
                auth="member", expect_status=422,
            ),
            # -- STT config lifecycle + validation --
            Request(
                "PUT", "/api/users/me/ai-assistant/stt-config/",
                route="/api/users/me/ai-assistant/stt-config/",
                json=dict(_STT_PUT),
                auth="member", expect_status=200,
            ),
            Request(
                "GET", "/api/users/me/ai-assistant/stt-config/",
                route="/api/users/me/ai-assistant/stt-config/",
                auth="member", expect_status=200,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/stt-config/",
                route="/api/users/me/ai-assistant/stt-config/",
                json={"model_name": "whisper-large"},
                auth="member", expect_status=200,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/stt-config/",
                route="/api/users/me/ai-assistant/stt-config/",
                json=dict(_STT_PUT, model_name=""),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/stt-config/",
                route="/api/users/me/ai-assistant/stt-config/",
                json=dict(_STT_PUT, base_url="ftp://x/v1"),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/stt-config/",
                route="/api/users/me/ai-assistant/stt-config/",
                json=dict(_STT_PUT, api_key="short"),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/stt-config/",
                route="/api/users/me/ai-assistant/stt-config/",
                json=dict(_STT_PUT, base_url="http://127.0.0.1:9000/v1"),
                auth="member", expect_status=400,
            ),
            Request(
                "PUT", "/api/users/me/ai-assistant/stt-config/",
                route="/api/users/me/ai-assistant/stt-config/",
                json=dict(_STT_PUT),
                auth="admin", expect_status=200,
            ),
            # -- transcribe with config set: file required, auth --
            Request(
                "POST", "/api/users/me/ai-assistant/transcribe/",
                route="/api/users/me/ai-assistant/transcribe/",
                auth="member", expect_status=400,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/transcribe/",
                route="/api/users/me/ai-assistant/transcribe/",
                files={"file": _AUDIO},
                auth="anonymous", expect_status=401,
            ),
            # -- MCP servers: create, credential lifecycle, validation --
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Tools", "url": _MCP_URL},
                auth="member", capture={"mcp1": "id"},
                expect_status=201,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Secured", "url": _MCP_URL,
                      "auth_header": "Bearer s3cret-value"},
                auth="member", capture={"mcp2": "id"},
                expect_status=201,
            ),
            Request(
                "PATCH", "/api/users/me/ai-assistant/mcp-servers/{mcp2}/",
                route="/api/users/me/ai-assistant/mcp-servers/{}/",
                json={"name": "Renamed"},
                auth="member", expect_status=200,
            ),
            Request(
                "PATCH", "/api/users/me/ai-assistant/mcp-servers/{mcp2}/",
                route="/api/users/me/ai-assistant/mcp-servers/{}/",
                json={"auth_header": ""},
                auth="member", expect_status=200,
            ),
            Request(
                "PATCH", "/api/users/me/ai-assistant/mcp-servers/{mcp1}/",
                route="/api/users/me/ai-assistant/mcp-servers/{}/",
                json={"is_enabled": False},
                auth="member", expect_status=200,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Bad-1", "url": "ftp://tools.example.com/mcp"},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Bad-2", "url": "javascript:alert(1)"},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Bad-3", "url": "https://user:pw@8.8.8.8/mcp"},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Bad-4", "url": ""},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Tools", "url": "https://8.8.8.9/b"},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "First", "url": _MCP_URL},
                auth="member", capture={"mcp_first": "id"},
                expect_status=201,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Second", "url": _MCP_URL},
                auth="member", capture={"mcp_second": "id"},
                expect_status=201,
            ),
            Request(
                "PATCH", "/api/users/me/ai-assistant/mcp-servers/{mcp_second}/",
                route="/api/users/me/ai-assistant/mcp-servers/{}/",
                json={"name": "First"},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Private", "url": _MCP_URL},
                auth="admin", capture={"mcp_priv": "id"},
                expect_status=201,
            ),
            Request(
                "PATCH", "/api/users/me/ai-assistant/mcp-servers/{mcp_priv}/",
                route="/api/users/me/ai-assistant/mcp-servers/{}/",
                json={"name": "Hijacked"},
                auth="member", expect_status=404,
            ),
            Request(
                "DELETE", "/api/users/me/ai-assistant/mcp-servers/{mcp_priv}/",
                route="/api/users/me/ai-assistant/mcp-servers/{}/",
                auth="member", expect_status=404,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/mcp-servers/",
                route="/api/users/me/ai-assistant/mcp-servers/",
                json={"name": "Internal", "url": "http://127.0.0.1:9000/mcp"},
                auth="member", expect_status=400,
            ),
            Request(
                "DELETE", "/api/users/me/ai-assistant/mcp-servers/{mcp1}/",
                route="/api/users/me/ai-assistant/mcp-servers/{}/",
                auth="member", expect_status=204,
            ),
            # -- threads: create, isolation, patch/delete --
            Request(
                "POST", _THREADS + "/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                json={"title": "Hi"},
                auth="member", capture={"thread1": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _THREADS + "/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                json={},
                auth="member", capture={"thread_empty": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _THREADS + "/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                json={"title": "x" * 500},
                auth="member", capture={"thread_long": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _OTHER_BASE + "/threads/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                json={"title": "x"},
                auth="member", expect_status=403,
            ),
            Request(
                "POST", _THREADS + "/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                json={"title": "admin"},
                auth="admin", capture={"thread_admin": "id"},
                expect_status=201,
            ),
            Request(
                "PATCH", _THREADS + "/{thread_admin}/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/",
                json={"title": "hijack"},
                auth="member", expect_status=404,
            ),
            Request(
                "DELETE", _THREADS + "/{thread_admin}/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/",
                auth="member", expect_status=404,
            ),
            Request(
                "POST", _THREADS + "/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                json={"title": "draft"},
                auth="member", capture={"thread_draft": "id"},
                expect_status=201,
            ),
            Request(
                "PATCH", _THREADS + "/{thread_draft}/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/",
                json={"title": "final", "is_archived": True},
                auth="member", expect_status=200,
            ),
            Request(
                "DELETE", _THREADS + "/{thread_draft}/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/",
                auth="member", expect_status=204,
            ),
            Request(
                "PATCH", _THREADS + "/" + _MISSING + "/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/",
                json={"title": "x"},
                auth="member", expect_status=404,
            ),
            Request(
                "POST", _THREADS + "/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                json={"title": "s"},
                auth="guest", expect_status=403,
            ),
            # -- messages: queued turn + auto-title, brake, validation --
            Request(
                "POST", _THREADS + "/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                json={"title": ""},
                auth="member", capture={"chat1": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _THREADS + "/{chat1}/messages/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/messages/",
                json={"content": "do a thing"},
                auth="member", capture={"turn1": "turn/id", "msg1": "message/id"},
                expect_status=202,
            ),
            Request(
                "POST", _THREADS + "/{chat1}/messages/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/messages/",
                json={"content": "again"},
                auth="member", expect_status=409,
            ),
            Request(
                "POST", _THREADS + "/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                json={"title": "v"},
                auth="member", capture={"chat2": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _THREADS + "/{chat2}/messages/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/messages/",
                json={"content": "   "},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", _THREADS + "/{chat2}/messages/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/messages/",
                json={"content": "x" * 32001},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", _THREADS + "/{thread_admin}/messages/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/messages/",
                json={"content": "hi"},
                auth="member", expect_status=404,
            ),
            Request(
                "POST", _THREADS + "/" + _MISSING + "/messages/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/messages/",
                json={"content": "hi"},
                auth="guest", expect_status=403,
            ),
            # Thread list: verifies the first-message auto-title and
            # reaps the seeded stale empty thread on both sides.
            Request(
                "GET", _THREADS + "/",
                route="/api/workspaces/{}/ai-assistant/threads/",
                auth="member", expect_status=200,
            ),
            # -- cancel: active turn, idle thread, missing thread --
            Request(
                "POST", _THREADS + "/{chat1}/cancel/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/cancel/",
                auth="member", expect_status=204,
            ),
            Request(
                "POST", _THREADS + "/{chat2}/cancel/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/cancel/",
                auth="member", expect_status=409,
            ),
            Request(
                "POST", _THREADS + "/" + _MISSING + "/cancel/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/cancel/",
                auth="member", expect_status=404,
            ),
            # A post after the cancel still trips the brake: the
            # cancelled turn counts as active. A second cancel is
            # idempotent success.
            Request(
                "POST", _THREADS + "/{chat1}/messages/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/messages/",
                json={"content": "run"},
                auth="member", expect_status=409,
            ),
            Request(
                "POST", _THREADS + "/{chat1}/cancel/",
                route="/api/workspaces/{}/ai-assistant/threads/{}/cancel/",
                auth="member", expect_status=204,
            ),
            # -- teardown: config deletes (lifecycle close) --
            Request(
                "DELETE", "/api/users/me/ai-assistant/config/",
                route="/api/users/me/ai-assistant/config/",
                auth="member", expect_status=204,
            ),
            Request(
                "DELETE", "/api/users/me/ai-assistant/stt-config/",
                route="/api/users/me/ai-assistant/stt-config/",
                auth="member", expect_status=204,
            ),
            # -- desktop-only agent token: denial, never a credential --
            Request(
                "POST", "/api/users/me/ai-assistant/agent-token/",
                route="/api/users/me/ai-assistant/agent-token/",
                auth="member", expect_status=403,
            ),
            Request(
                "POST", "/api/users/me/ai-assistant/agent-token/",
                route="/api/users/me/ai-assistant/agent-token/",
                auth="anonymous", expect_status=401,
            ),
        ),
    ),
)
