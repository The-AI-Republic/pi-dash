"""Oracle contract suite for the CLI device flow (PIDASHCONV-103).

Covers the six ``pi_dash/api/urls/auth.py`` routes the installed runner
(``runner/src/cli/auth/login.rs``) drives during ``pidash auth login``.
Every test drives the live server over HTTP and asserts the exact status
codes and response shapes Django produces today. Shared helpers come from
``_harness`` (``Seed``/``env``); suite-local HTTP glue lives in
``conftest.py``. Session auth for ``approve`` uses a forged ``sessions``
row (``Seed.session_cookie``) sent as an explicit ``Cookie`` header — the
same cookie mechanism the server accepts for browser sessions (the
desktop app approves with its own live session).
"""
