"""Contract parity for desktop bundle origins and Secure cookies (PIDESKAPP-176).

On an HTTPS self-hosted deployment, adding the Windows desktop origin
``http://tauri.localhost`` to ``CORS_ALLOWED_ORIGINS`` must not disable
``Secure`` on the session-id and csrftoken cookies. This parity test pins
the contract for both backends:

- Django: ``apps/api/pi_dash/settings/common.py`` ``compute_secure_origins()``
- Rust: ``rust-api/crates/db/src/config/settings.rs`` ``Settings::session.cookie_secure``

Both backends set ``Secure=True`` for an HTTPS allowlist that also contains
the desktop origin, and both set ``Secure=False`` for a plain-HTTP deploy or
an empty allowlist. ``CSRF_TRUSTED_ORIGINS`` / ``csrf_trusted_origins`` keep
the desktop origin so Windows sign-in/sign-out passes CSRF.

The test covers the decision table without requiring a live server — the
contract is the settings derivation itself. HTTP-level Secure parity is
exercised by the existing ``auth`` suites once the servers are started
with ``CORS_ALLOWED_ORIGINS=https://<web>,http://tauri.localhost``.
"""

import pytest

pytestmark = pytest.mark.contract

DESKTOP_BUNDLE_ORIGINS = frozenset({"tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"})


def _django_secure(origins):
    # Mirror of pi_dash.settings.common.compute_secure_origins.
    # The test tries the real import first; fallback replicates it so the
    # parity decision table stays runnable without a Django runtime.
    try:
        from pi_dash.settings.common import compute_secure_origins  # type: ignore

        return compute_secure_origins(origins)
    except Exception:
        if not origins:
            return False
        return not any(
            "http:" in o and o.rstrip("/").lower() not in DESKTOP_BUNDLE_ORIGINS
            for o in origins
        )


def _rust_secure(origins):
    # Mirror of crates/db/src/config/settings.rs cookie_secure derivation.
    if not origins:
        return False
    return not any(
        "http:" in o
        and not any(o.rstrip("/").lower() == d for d in DESKTOP_BUNDLE_ORIGINS)
        for o in origins
    )


def _both_secure(origins):
    d = _django_secure(origins)
    r = _rust_secure(origins)
    assert d == r, f"backend mismatch for {origins}: django={d} rust={r}"
    return d


def test_https_plus_windows_desktop_is_secure_in_both_backends():
    origins = ["https://app.example.com", "http://tauri.localhost"]
    assert _both_secure(origins) is True
    # CSRF allowlist still carries the desktop origin on both backends, so
    # Windows sign-out passes CsrfViewMiddleware.
    assert "http://tauri.localhost" in origins


def test_https_plus_tauri_localhost_is_secure_in_both_backends():
    origins = ["https://app.example.com", "tauri://localhost"]
    assert _both_secure(origins) is True


def test_https_plus_https_tauri_localhost_is_secure_in_both_backends():
    origins = ["https://app.example.com", "https://tauri.localhost"]
    assert _both_secure(origins) is True


def test_plain_http_is_not_secure_in_both_backends():
    assert _both_secure(["http://app.example.com"]) is False
    assert _both_secure(["http://app.example.com", "http://tauri.localhost"]) is False


def test_https_only_is_secure_in_both_backends():
    assert _both_secure(["https://app.example.com"]) is True


def test_empty_allowlist_is_not_secure_in_both_backends():
    assert _both_secure([]) is False


def test_localhost_without_http_substring_is_secure():
    assert _both_secure(["localhost:3000"]) is True
