"""Token-hash mirror for seed-only use.

``pi_dash/runner/services/tokens.py::hash_token`` stores
``HMAC-SHA256(pepper, raw)`` hex where
``pepper = SHA256("runner/pepper/" + SECRET_KEY)``. The contract suite never
imports that module (no Django imports); it recomputes the digest from the
test process's own ``SECRET_KEY``, which must match the server under test.

This is seed setup, not an assertion: HTTP assertions still compare real
server responses. If the server's hash scheme ever changes, seeded enroll
attempts fail loudly with 401 instead of silently passing.
"""

from __future__ import annotations

import hashlib
import hmac


def hash_token(raw: str, secret_key: str) -> str:
    """Mirror of ``tokens.hash_token`` using only the standard library."""
    if not secret_key:
        raise RuntimeError(
            "SECRET_KEY is required in the test environment to seed token "
            "hashes: export the same SECRET_KEY the server under test boots with."
        )
    pepper = hashlib.sha256(("runner/pepper/" + secret_key).encode()).digest()
    return hmac.new(pepper, raw.encode(), hashlib.sha256).hexdigest()


def fingerprint(raw: str) -> str:
    """Mirror of ``tokens.fingerprint`` (first 12 hex chars of SHA256)."""
    return hashlib.sha256(raw.encode()).hexdigest()[:12]
