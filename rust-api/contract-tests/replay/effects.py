# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Explicit ignore maps for write-replay comparison (PIDASHCONV-823).

Every value the comparer blanks is named here (or in a session's own
``timestamp_columns`` / ``generated_columns`` / ``encrypted_columns``
additions, which name the table and column). Nothing is auto-detected:
a column whose values legitimately differ between backends but is not
listed here fails the replay loudly, and the fix is a precise map entry
with cited evidence — never a looser rule. This mirrors the shadow-read
volatile allowlist policy (``crates/api/src/edge_shadow.rs``: "extend
only with cited evidence, never speculatively").

Normalisation is type-preserving: ``None`` stays ``None`` and booleans
stay booleans, so "field went null on one side" still fails. Only
string/number/datetime scalars in a listed column/key become the
placeholder.
"""

from __future__ import annotations

import re
import time
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

# HTTP methods that count as writes (route_inventory.TRACKED_METHODS subset).
WRITE_METHODS = frozenset({"POST", "PUT", "PATCH", "DELETE"})

# Placeholder substituted for every normalised scalar. Type-preserving:
# None/bool pass through untouched (see _placeholder_for).
VOLATILE_PLACEHOLDER = "<replay-volatile>"

# --- Response-body normalisation (mirrors edge_shadow.rs) ---
# Exact copies of VOLATILE_JSON_KEYS / VOLATILE_QUERY_PARAMS in
# rust-api/crates/api/src/edge_shadow.rs. test_meta.py pins them: if the
# Rust side grows a key, this module must adopt it deliberately (with the
# same cited evidence) instead of drifting silently.
VOLATILE_JSON_KEYS = frozenset({"request_id", "requestId"})
VOLATILE_QUERY_PARAMS = frozenset(
    {
        "x-amz-date",
        "x-amz-expires",
        "x-amz-signature",
        "expires",
        "signature",
        "sig",
    }
)

# --- JSON body timestamp keys ---
# DRF renders these as server-time ISO strings that legitimately differ
# by the seconds between the two backends' requests. Matched exactly,
# case-sensitively, at any nesting depth; only string values are
# blanked (nulls and numbers still compare — a missing stamp is a bug).
TIMESTAMP_JSON_KEYS = frozenset(
    {
        "created_at",
        "updated_at",
        "date_joined",
        "last_login",
        "last_used",
        "expire_date",
        "expires_at",
        "completed_at",
        "archived_at",
        "deleted_at",
        # Evidence: comment PATCH stamps edited_at=now() on both backends
        # (replay app_issues_lifecycle request 4).
        "edited_at",
        # Evidence: loop job create defaults dtstart=now() when the
        # client sends none (admin_views.py:121-122), milliseconds
        # apart per backend (replay loop_lifecycle request 22). No
        # corpus sends dtstart explicitly, so every compared value is
        # server-minted now().
        "dtstart",
        # Evidence: set-password serializes the user with stamped
        # last_active/last_login_time/token_updated_at, seconds apart
        # (replay auth_session_lifecycle request 24); mirrors the DB map.
        "last_active",
        "last_login_time",
        "last_logout_time",
        "token_updated_at",
        "createdAt",
        "updatedAt",
        "dateJoined",
        "lastLogin",
        # Evidence: the web chat message-accept path writes a
        # chat_timing event whose JSONB payload nests recorded_at
        # (replay runner_web_lifecycle request 17); stamped now()
        # per backend like every other _at key here.
        "recorded_at",
        # Evidence: runner detail bodies render enrolled_at, minted
        # at enroll/create per backend (replay runner_web_lifecycle
        # request 28). No corpus sends it (DB presence still guarded
        # by the timestamp_columns map), so every compared value is
        # server-minted now().
        "enrolled_at",
        # Evidence: the runner-revoke body renders the just-stamped
        # revoked_at (replay runner_web_lifecycle request 32); same
        # server-minted class, DB-guarded the same way.
        "revoked_at",
        # Evidence: the agent-run yield stamps done_payload.yielded_at
        # = now() per backend (api/views/issue.py:1303); the replay
        # sees two instants milliseconds apart (v1_work_items_followup
        # request 31). No corpus sends it.
        "yielded_at",
        # Evidence: notification mark-read stamps read_at = now() per
        # backend (notification/base.py:166, :285-287); the replay
        # sees two instants seconds apart (app_notifications_lifecycle
        # requests 12+). The key exists only in the notification
        # model/serializer, so every compared value is server-minted.
        "read_at",
        # Evidence: PAT disconnect stamps config.disconnected_at =
        # now() per backend (integration/github.py:515); the replay
        # sees two instants milliseconds apart inside the JSONB cell
        # (app_integrations_lifecycle request 21). The key exists
        # only there, so every compared value is server-minted.
        "disconnected_at",
    }
)

# --- Database timestamp columns ---
# Columns whose values are stamped with now() by the server on write.
# Same type-preserving rule as TIMESTAMP_JSON_KEYS. Per-session extras
# (a domain's own stamp columns) go in Session.timestamp_columns.
TIMESTAMP_COLUMNS = frozenset(
    {
        "created_at",
        "updated_at",
        "date_joined",
        "last_login",
        "last_used",
        "expire_date",
        "completed_at",
        "archived_at",
        "deleted_at",
        # Evidence: comments.edited_at is stamped now() by comment PATCH
        # on both backends (replay app_issues_lifecycle request 4).
        "edited_at",
        # Evidence: sign-up stamps last_active/last_login_time/
        # token_updated_at with now() on both backends, seconds apart
        # (replay auth_session_lifecycle request 3); sign-out stamps
        # last_logout_time the same way (request 44).
        "last_active",
        "last_login_time",
        "last_logout_time",
        "token_updated_at",
        # Evidence: notification mark-read stamps read_at = now() per
        # backend (notification/base.py:166, :285-287); the column
        # exists only on notifications (app_notifications_lifecycle).
        "read_at",
        # Evidence: the App webhook stamps received_at (auto_now_add)
        # and processed_at = now() per backend
        # (db/models/integration/github.py:176-177); the replay sees
        # two instants seconds apart (app_integrations_lifecycle
        # request 63). Every writer is server-side (the view on
        # receipt/process, the email-notifications worker on its own
        # table), so every compared value is server-minted.
        "received_at",
        "processed_at",
    }
)

# --- Database generated-id columns ---
# Columns whose values are minted server-side per created row (uuid4 PKs
# and the like), so the two backends can never agree byte for byte.
# ``id`` is the uuid PK convention across the Django schema. FK columns
# that reference *seeded* rows are deterministic and are NOT listed —
# they still compare. FKs that reference *created* rows (whose ids
# differ per backend) are aligned through the session's capture map,
# not through this list. Per-session extras (a domain's own minted
# columns, e.g. token/key columns) go in Session.generated_columns
# with the generating view cited.
GENERATED_ID_COLUMNS = frozenset({"id"})

# --- Skipped tables ---
# Present on one side only (or compared through a dedicated path), with
# the reason. Anything else present on exactly one side fails loudly.
SKIP_TABLES = frozenset(
    {
        # Rust-owned runtime DDL (queue::ensure_schema, worker boot):
        # absent from the Django schema by design. Enqueued jobs are
        # compared through the jobs path (compare_jobs), not the DB path.
        "rust_job_queue",
        "rust_job_schedule",
    }
)

# --- Sessions table ---
# Django signs session_data with a per-mint timestamp and the session
# key is random per backend (forge_session_key), so rows can never match
# field for field. What the replay pins instead: the multiset of
# user_ids (seeded users are deterministic, so a sign-in on one side
# only — or for the wrong user — still fails) plus the row count, which
# the multiset length carries. Ids no capture names (view-created users:
# sign-up mints a fresh uuid per backend) must exist as users rows on
# their own side and then compare by count (see compare.py).
SESSIONS_TABLE = "sessions"
SESSIONS_COMPARE_KEYS = ("user_id",)

# --- Webhook sink URLs ---
# Seeded webhook rows point at a placeholder host; snapshot.rewrite
# rewrites it per backend to that backend's sink. Comparison blanks the
# base of any URL found in these columns so the per-backend hosts do not
# count as a difference (path + query still compare).
WEBHOOK_URL_COLUMNS = frozenset({("webhooks", "url")})

# --- Soft-deleted workspace slugs ---
# Workspace.delete() rewrites the slug to f"{slug}__{int(deleted_at.timestamp())}"
# (db/models/workspace.py:156-174; the Rust port mirrors the format in
# services/src/app_workspace/models_workspace.rs soft_delete_slug). The replay
# sends each request to Django first, then Rust, so when a wall-clock second
# boundary falls between the two DELETEs the suffixes differ by exactly one
# (app_workspace_lifecycle request 48: rp-wsa__1791814527 vs rp-wsa__1791814528,
# PIDASHCONV-1005). Comparison blanks the suffix — and only the suffix — when
# it parses as a plausible deletion epoch: the prefix still compares (a backend
# that deleted the wrong workspace still fails), live slugs without the suffix
# compare exactly, and an implausible suffix (epoch 0, a far-future year, a
# user slug that merely ends in __digits outside the window) stays raw and
# fails loudly.
WORKSPACE_SLUG_TABLE = "workspaces"
WORKSPACE_SLUG_COLUMN = "slug"
SOFT_DELETE_SLUG_RE = re.compile(r"^(?P<prefix>.+)__(?P<epoch>\d+)$")
# The suffix must fall in this window around now: the compare runs seconds
# after the DELETE and a session lasts minutes, so an hour back absorbs slow
# CI while still rejecting epoch 0 / far-future garbage (mirrors the
# plausibly-now gate job-payload timestamps get in compare._blank_time_paths).
SOFT_DELETE_EPOCH_LOOKBACK_SECONDS = 3600
SOFT_DELETE_EPOCH_SKEW_SECONDS = 300

# --- Job comparison ---
# Celery message ids (and Rust celery_id) are minted per publish and
# carry no behaviour: dropped before comparison. ETA / visible_at embed
# each backend's own now(): both-absent compares equal, otherwise the
# absolute datetimes must agree within this tolerance. Countdown-based
# ETAs legitimately differ by the seconds between the two requests;
# 120s also absorbs slow CI runners while still catching a dropped ETA
# (absent vs present fails) or a wrong delay (hours apart fails).
JOB_ETA_TOLERANCE_SECONDS = 120.0

# Placeholder host seeded into webhook rows; rewritten per backend.
WEBHOOK_PLACEHOLDER_HOST = "replay-webhook-sink.invalid"

# --- Password-reset tokens in job payloads ---
# ``forgot_password.delay`` carries a freshly minted reset token
# (``PasswordResetTokenGenerator``: base36 seconds + HMAC). The two
# backends mint seconds apart, so the bytes can never match — but a
# blind blank would also hide a broken HMAC. The comparer instead
# VALIDATES each side's token against its own user row (same recipe as
# ``auth_session/tokens.py`` minting plus Django's ``check_token`` age
# gate) and blanks it only when valid; anything else fails loudly.
# Timeout mirrors ``settings/common.py`` (3600, not Django's default).
RESET_TOKEN_TIMEOUT_SECONDS = 3600
RESET_TOKEN_KEY_SALT = "django.contrib.auth.tokens.PasswordResetTokenGenerator"

# --- process_logs payload topology ---
# The API-token audit payload embeds the request's Host header, which
# legitimately differs per backend by port in the replay topology
# (127.0.0.1:18223 vs :18225). Blank only the host:port *values* inside
# this task's payloads; every other field still compares.
PROCESS_LOGS_TASK = "pi_dash.bgtasks.logger_task.process_logs"
TOPOLOGY_HOST_RE = re.compile(r"127\.0\.0\.1:\d+")
# Content-Length *value* inside the headers repr (the key stays: its
# presence differs on bodyless requests). Single-quote repr form —
# the payload is Python str(dict), never JSON.
CONTENT_LENGTH_RE = re.compile(r"('Content-Length': ')[^']*(')")


def placeholder_for(value):
    """Type-preserving placeholder: None/bool pass through, else blank."""
    if value is None or isinstance(value, bool):
        return value
    return VOLATILE_PLACEHOLDER


def _b36_to_int(value: str) -> int:
    return int(value, 36)


def validate_reset_token(
    token: str,
    user_id: str,
    password_field: str,
    last_login,
    email: str,
    secret: str,
    now=None,
) -> bool:
    """Whether ``token`` is a valid reset token for this user row.

    Mirrors Django 4.2 ``PasswordResetTokenGenerator.check_token`` with
    the ``auth_session/tokens.py`` value recipe: base36 timestamp within
    :data:`RESET_TOKEN_TIMEOUT_SECONDS` of now, and the HMAC half
    recomputed from ``(user_id, password_field, last_login, ts, email)``
    matching. ``last_login`` is the raw DB value (None or datetime).
    """
    import hashlib
    import hmac
    from datetime import datetime, timezone

    if not isinstance(token, str) or "-" not in token:
        return False
    ts_b36, _, _hash = token.partition("-")
    try:
        ts = _b36_to_int(ts_b36)
    except ValueError:
        return False
    if now is None:
        now = datetime.now(timezone.utc).replace(tzinfo=None)
    epoch = datetime(2001, 1, 1)
    age = int((now - epoch).total_seconds()) - ts
    if age < 0 or age > RESET_TOKEN_TIMEOUT_SECONDS:
        return False
    if last_login is None:
        login_ts = ""
    else:
        moment = last_login
        if getattr(moment, "tzinfo", None) is not None:
            moment = moment.replace(tzinfo=None)
        login_ts = str(moment.replace(microsecond=0))
    value = f"{user_id}{password_field}{login_ts}{ts}{email}"
    key = hashlib.sha256((RESET_TOKEN_KEY_SALT + secret).encode()).digest()
    expect = hmac.new(key, msg=value.encode(), digestmod=hashlib.sha256).hexdigest()[::2]
    return hmac.compare_digest(expect, token.partition("-")[2])


_FERNET_CACHE: dict = {}
_FERNET_DIRECT_CACHE: dict = {}


def fernet_decrypt_direct(value, keys: list[str]) -> str | None:
    """Decrypt one cell under raw Fernet keys (assistant BYOK schedule).

    Mirrors ``pi_dash/assistant/crypto.py`` ``FernetBackend``: the keys
    are used directly (MultiFernet — any key decrypts), no PBKDF2.
    ``value`` is the frozen cell: hex for BinaryField bytes (see
    ``compare.freeze``), possibly a raw str; returns the plaintext str,
    or None when undecryptable (callers leave such cells raw so they
    fail loudly like any other cell).
    """
    if not value or not keys:
        return None
    try:
        from cryptography.fernet import Fernet, MultiFernet

        cache_key = "\x00".join(keys)
        cached = _FERNET_DIRECT_CACHE.get(cache_key)
        if cached is None:
            cached = MultiFernet([Fernet(key.encode("utf-8")) for key in keys])
            _FERNET_DIRECT_CACHE[cache_key] = cached
        if isinstance(value, (bytes, bytearray, memoryview)):
            tokens = [bytes(value)]
        else:
            text = str(value)
            tokens = []
            try:
                tokens.append(bytes.fromhex(text))
            except ValueError:
                pass
            tokens.append(text.encode())
        for token in tokens:
            try:
                return cached.decrypt(token).decode("utf-8")
            except Exception:
                continue
        return None
    except Exception:
        return None


def fernet_decrypt(value: str, secret_key: str) -> str | None:
    """Decrypt one Fernet cell under Django's license key schedule.

    Mirrors ``pi_dash/license/utils/encryption.py`` (``derive_key`` +
    ``decrypt_data``): PBKDF2-HMAC-SHA256 over the server SECRET_KEY
    (100 000 rounds, salt ``b"salt"``), URL-safe base64, standard
    Fernet. The Rust port uses the same schedule
    (``crates/db/src/config/encryption.rs``), so either backend's
    ciphertext must decrypt here: Fernet randomises the IV, so two
    encryptions of the same plaintext never match byte for byte —
    comparing the decrypted plaintext is the only strict check.

    Returns None when ``value`` is not valid ciphertext under the key
    (a plaintext seed in an ``is_encrypted`` row, or a backend that
    stored garbage): callers leave such cells raw so they compare —
    and fail — exactly like any other cell.
    """
    if not value or not secret_key:
        return None
    try:
        import base64
        import hashlib

        from cryptography.fernet import Fernet

        # The key schedule is 100k PBKDF2 rounds; the secret is fixed
        # per replay run, so derive once and reuse across snapshots.
        cached = _FERNET_CACHE.get(secret_key)
        if cached is None:
            dk = hashlib.pbkdf2_hmac("sha256", secret_key.encode(), b"salt", 100000)
            cached = Fernet(base64.urlsafe_b64encode(dk))
            _FERNET_CACHE[secret_key] = cached
        return cached.decrypt(value.encode()).decode()
    except Exception:
        return None


def decode_reset_uid(uidb64: str):
    """Decode a reset uidb64 to the user id string, or None when malformed."""
    import base64

    if not isinstance(uidb64, str) or not uidb64:
        return None
    try:
        padded = uidb64 + "=" * (-len(uidb64) % 4)
        decoded = base64.urlsafe_b64decode(padded.encode()).decode()
    except (ValueError, UnicodeDecodeError):
        return None
    return decoded or None


def _redact_url_params(url: str) -> str:
    """Redact volatile query params inside a URL string (shadow rule)."""
    try:
        parts = urlsplit(url)
    except ValueError:
        return url
    if not parts.query:
        return url
    if "://" not in url and not url.startswith("/"):
        return url
    kept = [
        (k, v) for k, v in parse_qsl(parts.query, keep_blank_values=True)
        if k.lower() not in VOLATILE_QUERY_PARAMS
    ]
    redacted = [(k, VOLATILE_PLACEHOLDER) for k, v in parse_qsl(parts.query, keep_blank_values=True) if k.lower() in VOLATILE_QUERY_PARAMS]
    if not redacted:
        return url
    return urlunsplit(
        (parts.scheme, parts.netloc, parts.path, urlencode(kept + redacted), parts.fragment)
    )


_URL_LIKE = re.compile(r"^(https?://|/)", re.IGNORECASE)


def normalise_json_body(value, captures: dict[str, tuple] | None = None):
    """Normalise a decoded JSON body for cross-backend comparison.

    ``captures`` maps capture name -> (django_value, rust_value); each
    side's own values are replaced with ``{name}`` before comparing, so
    generated ids referenced through the session align. Which side's
    values to replace is chosen by ``side`` via :func:`bind_captures`.
    """
    by_value = captures or {}

    def walk(node):
        if isinstance(node, dict):
            return {
                key: (
                    placeholder_for(node[key])
                    if key in VOLATILE_JSON_KEYS
                    else (
                        placeholder_for(node[key])
                        if key in TIMESTAMP_JSON_KEYS and isinstance(node[key], str)
                        else walk(node[key])
                    )
                )
                for key in node
            }
        if isinstance(node, list):
            return [walk(item) for item in node]
        if isinstance(node, str):
            if node in by_value:
                return by_value[node]
            if _URL_LIKE.match(node):
                redacted = _redact_url_params(node)
                return by_value.get(redacted, redacted)
            return node
        return node

    return walk(value)


def bind_captures(capture_values: dict[str, tuple], side: str) -> dict:
    """Flatten {name: (django, rust)} to {own_value: "{name}"} for one side."""
    index = 0 if side == "django" else 1
    flat = {}
    for name, pair in capture_values.items():
        value = pair[index]
        if isinstance(value, str) and value:
            flat[value] = "{%s}" % name
    return flat


def soft_delete_slug_prefix(value):
    """Live-slug prefix of a soft-deleted workspace slug, else None.

    Returns the prefix (the slug the row carried before delete) when
    ``value`` has the ``{slug}__{epoch}`` shape with a plausible deletion
    epoch; None for live slugs and implausible suffixes (both compare raw).
    """
    if not isinstance(value, str):
        return None
    match = SOFT_DELETE_SLUG_RE.match(value)
    if not match:
        return None
    try:
        epoch = int(match.group("epoch"))
    except ValueError:
        return None
    now = int(time.time())
    if not (now - SOFT_DELETE_EPOCH_LOOKBACK_SECONDS <= epoch <= now + SOFT_DELETE_EPOCH_SKEW_SECONDS):
        return None
    return match.group("prefix")


def normalise_db_value(table: str, column: str, value, ts_extra=frozenset(), gen_extra=frozenset()):
    """Normalise one frozen DB cell for cross-backend comparison.

    ``value`` is already frozen (datetimes/UUIDs rendered to strings,
    like ``_harness.db._freeze``). ``ts_extra`` / ``gen_extra`` are the
    session's table-scoped additions to :data:`TIMESTAMP_COLUMNS` /
    :data:`GENERATED_ID_COLUMNS` for ``table``.
    """
    if table == WORKSPACE_SLUG_TABLE and column == WORKSPACE_SLUG_COLUMN:
        prefix = soft_delete_slug_prefix(value)
        if prefix is not None:
            return prefix + "__" + VOLATILE_PLACEHOLDER
        return value
    if column in TIMESTAMP_COLUMNS or column in ts_extra:
        if value is None or isinstance(value, bool):
            return value
        return VOLATILE_PLACEHOLDER
    if column in GENERATED_ID_COLUMNS or column in gen_extra:
        return placeholder_for(value)
    return value
