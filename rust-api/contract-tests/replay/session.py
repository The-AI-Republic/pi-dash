# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus model: sessions of ordered write requests (PIDASHCONV-823).

A session is one realistic multi-step flow (create → update → relate →
delete) against a seeded world. Requests are templates: ``{name}``
placeholders in the path, query params, headers, and JSON body resolve
per backend from values captured out of that backend's own earlier
responses, so generated ids (which legitimately differ per backend)
still line up request by request.

Capture pointers are ``/``-separated JSON pointers over the decoded
response body (``issue/id``; list indices as decimals). A request that
needs no captures needs no pointers.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass(frozen=True)
class Request:
    """One replayed HTTP request (a template, rendered per backend)."""

    method: str
    # Path template, e.g. "/api/issues/{issue_id}/". Placeholders resolve
    # from the running backend's capture context.
    path: str
    # Route template for coverage, e.g. "/api/issues/{pk}/". Defaults to
    # the path with every {capture} rewritten to {param}; set it
    # explicitly when the capture name differs from the route's param
    # name (coverage joins on shape, so names do not matter — but the
    # explicit form documents intent).
    route: str = ""
    # JSON body template (dict/list/str with {placeholders}); None sends
    # no body. Raw-string bodies (non-JSON posts) use raw_body instead;
    # HTML-form posts use form instead; multipart uploads use files
    # instead. At most one of json / raw_body / form / files may be set
    # (the runner rejects a request that sets two).
    json: object = None
    raw_body: bytes | None = None
    # Form fields template ({placeholders} in values); sent urlencoded.
    # The /auth/* plain-Django Views and the public asset POST only read
    # form data, so corpus flows through them need templated fields the
    # same way JSON posts do.
    form: dict = field(default_factory=dict)
    # Multipart files ({field: (filename, bytes, content_type)},
    # {placeholders} in filenames); sent as multipart/form-data (the
    # transcribe endpoint only reads uploads). Bytes pass through
    # templating untouched.
    files: dict = field(default_factory=dict)
    # Query params and extra headers (templates in values).
    params: dict = field(default_factory=dict)
    headers: dict = field(default_factory=dict)
    # Which credential to send: a key from Session.auth_users, "api_key"
    # (the seeded workspace token), "admin:<user>" (the same forged
    # session row presented as the admin-session-id cookie the
    # /api/instances/* console reads), "cookie:<capture>" (a cookie value
    # captured from an earlier response), or "anonymous".
    auth: str = "anonymous"
    # Extra cookies template ({name: value}, {placeholders} rendered per
    # side); merged into the Cookie header after the auth cookie. For the
    # CSRF-primed browser flows: {"csrftoken": "{csrf_cookie}"}.
    cookies: dict = field(default_factory=dict)
    # Seconds to sleep before sending (both sides, once). The device-code
    # token poll enforces a 3s minimum gap between polls (RFC 8628
    # slow_down); the corpus waits out the protocol instead of racing it.
    delay_before: float = 0.0
    # Captures: {name: pointer} into this response's decoded JSON body.
    capture: dict = field(default_factory=dict)
    # Expected Django status. The oracle is differential (Rust must equal
    # Django), but the author's expectation pins Django so a corpus bug
    # (bad seed, wrong payload) fails fast instead of locking in a 500.
    expect_status: int | None = None
    # Known job content divergences for THIS request only:
    # [(task, fix_issue, dj_match, rs_match)]. Both sides must emit the
    # task on this request and the normalised payloads must exactly
    # equal the pinned bytes (placeholders via {captures}, world/static
    # ctx); anything else — task missing on either side, or any byte
    # deviation — fails loudly (fix landed -> remove the pin; Django
    # changed -> investigate). Per-request because a task's content
    # legitimately varies per request (create emits no before-image,
    # PATCH does), so a session-wide exact pin would misfire.
    known_job_shapes: list = field(default_factory=list)
    # Known response-header divergences for THIS request only:
    # [(header, fix_issue, dj_value, rs_value)]. Each side's header
    # must exactly equal its pinned value (a template over the gap
    # ctx; None means the header must be ABSENT); any deviation fails
    # loudly (fix landed -> remove the pin; Django changed ->
    # investigate). Pinned headers skip the standard header checks.
    known_headers: list = field(default_factory=list)
    # Known JSON body divergences for THIS request only:
    # [(json_pointer, fix_issue, dj_value, rs_value)]. Each side's
    # subtree at the ``/``-separated pointer must exactly equal its
    # pinned value (a template over the gap ctx); any deviation fails
    # loudly (fix landed -> remove the pin; Django changed ->
    # investigate). Pinned subtrees blank out of the diff.
    known_body_diffs: list = field(default_factory=list)
    # Known absent jobs for THIS request only: {task: fix_issue}. The
    # conditional counterpart to Session.known_absent_jobs (which
    # applies to every request and so only fits unconditional emits):
    # Django must emit the task on this request and Rust must not;
    # either side changing fails loudly. Pinned tasks are excluded
    # from the job multiset compare.
    known_absent_jobs: dict = field(default_factory=dict)
    # Volatile JSON body subtrees for THIS request only: [pointer, ...]
    # (``/``-separated). Each pointer must resolve on BOTH sides (a
    # missing side fails loudly, so this never silently excuses a
    # dropped field) and both values blank out of the diff. For
    # independently-random bytes neither side can predict — e.g. an
    # S3 presigned POST whose key uuid, policy and signature all
    # derive from per-backend randomness — where a capture (exact
    # occurrences of one response value) cannot reach.
    body_blank_paths: list = field(default_factory=list)
    # Regex blanks over string leaves for THIS request only: [pattern,
    # ...] (Python ``re``). Each pattern must match at least once on
    # EACH side (a side with no match fails loudly) and every match
    # blanks to the volatile placeholder. For server-minted values
    # embedded inside larger strings — e.g. a per-render run id woven
    # through a 70 KB prompt — where no JSON pointer can isolate the
    # value. Anchor patterns to their surrounding context (never a
    # bare uuid shape) so stable ids in the same text still compare.
    body_regex_blanks: list = field(default_factory=list)
    # Skip the response body-bytes (and content-type) compare for THIS
    # request only. For framework-rendered error scaffolding whose bytes
    # are environment-defined, not behaviour: Django's DEBUG 500 page
    # embeds the request host/port, settings (DB names, redis DBs, SMTP
    # ports, per-process keys) and memory addresses, while the Rust port
    # answers its generic JSON 500 — the same exception class either
    # way (production runs DEBUG=False, where Django is generic too).
    # Status, location, and cookies still compare, and every
    # body_contains pin must match; DB, jobs, and sinks compare fully.
    # This mirrors the contract suite, which pins status-only (plus the
    # persisted DB half) on exactly these endpoints.
    skip_body: bool = False
    # Required presence pins when skip_body is set: [(dj_substr,
    # rs_substr), ...]. Each side's raw body text must contain its
    # substring (templates over the gap ctx); a missing side fails
    # loudly. Names the exception kind the skipped page renders (e.g.
    # "<title>ValidationError"), so a backend that starts failing a
    # DIFFERENT way still fails the replay.
    body_contains: list = field(default_factory=list)
    # Known DB cell divergences introduced BY this request: [(table,
    # fix_issue, match_dict, column, dj_value, rs_value)] — same shape
    # as Session.known_cell_diffs. Unlike body/job pins, cell pins
    # ACCUMULATE: once declared they apply to this request and every
    # later one, because the divergent state persists like any other
    # state. Declare the pin on the request that introduces the
    # divergence; that is the only window a session-wide pin cannot
    # express (a matcher cannot tell a live row's pre-write state
    # from its post-write state when no observable column changes).
    known_cell_diffs: list = field(default_factory=list)
    # Known job-payload value divergences for THIS request only:
    # {task_name_or_"*": [(pointer, fix_issue, dj_value, rs_value)]} —
    # same shape as Session.payload_value_diffs, but scoped to one
    # request. For values that collide with agreed states elsewhere
    # in the session (a before-image pin whose Django value another
    # request's jobs also carry while Rust agrees): a session-wide
    # pin would blank one side and manufacture a divergence.
    payload_value_diffs: dict = field(default_factory=dict)

    def route_template(self) -> str:
        if self.route:
            return self.route
        import re

        return re.sub(r"\{[^}]+\}", "{param}", self.path)


@dataclass(frozen=True)
class Session:
    """One ordered request list plus its world and comparison tuning."""

    # Unique across the corpus (corpus/<domain>_<flow>); the pytest id.
    name: str
    # Contract-test domain this session's flow is built from.
    domain: str
    # Human provenance: suite files / fixtures the flow derives from.
    source: str
    requests: tuple
    # Users the session needs, as keys into the world seed
    # (corpus.world.USERS) plus the "api_key" pseudo-credential.
    auth_users: tuple = ("admin",)
    # Extra per-table comparison tuning, all explicit: {table: [column]}.
    # timestamp_columns: domain stamp columns beyond TIMESTAMP_COLUMNS.
    # generated_columns: domain minted-value columns beyond {"id"}.
    # encrypted_columns: {table: [column | (column, "assistant")]}.
    # Fernet-ciphertext columns comparing by DECRYPTED plaintext (the
    # IV is random per encryption, so ciphertext never matches). Bare
    # columns use the license schedule (PBKDF2 over SECRET_KEY) gated
    # on a true row ``is_encrypted`` flag; ("assistant") columns use
    # the raw ASSISTANT_ENCRYPTION_KEY list with no flag gate. Rows
    # outside the gate, and undecryptable cells, compare raw.
    timestamp_columns: dict = field(default_factory=dict)
    generated_columns: dict = field(default_factory=dict)
    encrypted_columns: dict = field(default_factory=dict)
    # Extra tables to skip in DB comparison: {table: reason}.
    skip_tables: dict = field(default_factory=dict)
    # JSON-pointer prefixes inside job args/kwargs that carry timestamps
    # (task id, delay seconds); values there compare within the ETA
    # tolerance instead of exactly. {task_name_or_"*": [pointer, ...]}.
    payload_time_paths: dict = field(default_factory=dict)
    # Extra seed for this session: fn(conn) run once against the Django
    # copy after the base restore; the runner clones the result into the
    # Rust copy, so the function may use random ids and now() freely.
    # None means the base world seed is enough.
    seed: object = None
    # Static template context: {placeholder: value}, identical on both
    # backends (for fixed ids the session seed wrote that requests
    # address, e.g. a move-target project id).
    static_context: dict = field(default_factory=dict)
    # Extra per-run context: fn(world, config) -> dict, called once after
    # restore + seed + clone, merged into the template context. For values
    # only knowable at run time: ids the seed generated (read them back
    # from the Django copy — the clone is identical), minted tokens (the
    # password-reset HMAC needs the live user row), and same-value Redis
    # priming both backends must observe (magic-link codes live only in
    # Redis and the generate response never reveals them). The function
    # may write to both backends' Redis caches (config cache urls) but
    # must NOT write to either database (the copies already match).
    setup: object = None
    # Placeholder names setup() provides (declared so the manifest check
    # can validate them without a live database; the runner fails loudly
    # when setup() omits one).
    setup_keys: tuple = ()
    # Known job divergences: {task_name: fix_issue_id}. The task must be
    # emitted by Django and NOT by Rust on every request; either side
    # changing fails loudly (fix landed -> remove the pin; Django
    # changed -> investigate). Pinned tasks are excluded from the job
    # multiset compare. For unconditional emits only (middleware); a
    # conditional pin needs a presence mode this map does not have yet.
    known_absent_jobs: dict = field(default_factory=dict)
    # Job payload row references: {task_name_or_"*": [(pointer, table)]}.
    # Values at these pointers are backend-local row ids (never exposed
    # in a response, so uncapturable). Each side's value must EXIST as
    # that table's id in its own database (checked live); existing rows
    # compare as {row:<table>}, missing rows fail loudly. Pointers are
    # multi-segment over dicts/lists and descend through nested JSON
    # strings (kwargs/current_instance/created_x/0/pk).
    payload_row_refs: dict = field(default_factory=dict)
    # Job payload known-value divergences: {task_name_or_"*":
    # [(pointer, fix_issue, dj_value, rs_value)]}. Values at these
    # pointers are backend-local scalars a filed divergence explains
    # (a wrongly-minted counter echoed inside current_instance — not
    # row ids, those are payload_row_refs). Each side's value blanks
    # only when it exactly equals its pinned literal; anything else
    # stays and fails loudly. Pointers descend through nested JSON
    # strings like payload_row_refs. Session-level: when several rows
    # diverge with different values, declare one pin per value — each
    # blanks only its exact match, so pins coexist safely.
    payload_value_diffs: dict = field(default_factory=dict)
    # Database cell row references: [(table, column, ref_table, pointer)].
    # Same existence-checked blanking for DB cells: scalar FK-like
    # generated values (pointer "") or values nested in a JSONB column
    # (pointer "a/b/0"). Each side's value must exist as ref_table's id.
    db_cell_refs: list = field(default_factory=list)
    # Validated reset tokens in job payloads: {task_name_or_"*":
    # [(uid_pointer, token_pointer)]}. Pointers are args/... or
    # kwargs/... roots. Each side's token is validated against its own
    # user row (decoded from the uid pointer) and blanked only when
    # valid — a broken HMAC, expired stamp, or wrong user fails loudly
    # instead of comparing uncomparable bytes.
    payload_reset_tokens: dict = field(default_factory=dict)
    # Validated magic codes in job payloads: {task_name_or_"*":
    # [(email_pointer, code_pointer)]}. Each side's queued code must
    # equal the token its own backend stored in its cache Redis
    # (``magic_<email>`` row); matches blank, anything else fails.
    payload_magic_codes: dict = field(default_factory=dict)
    # Known DB divergences: [(table, fix_issue, match_dict)]. Django-only
    # rows in `table` that exactly equal a rendered match_dict are
    # excused; anything else (unmatched django-only rows, any rust-only
    # rows) still fails. Every declaration must match at least one row
    # during the session, or the run fails telling the author to remove
    # it (the fix landed). match_dict holds the full normalised row
    # (placeholders via {captures}, world/static ctx, "<replay-volatile>"
    # for stamped/generated cells) so the pin is exact, not a shape.
    known_extra_rows: list = field(default_factory=list)
    # Known DB divergences, Rust side: [(table, fix_issue, match_dict)].
    # The exact mirror: Rust-only rows in `table` that exactly equal a
    # rendered match_dict are excused (a leaked row the Rust port writes
    # but Django deletes); unmatched rust-only rows and any django-only
    # rows still fail, and every declaration must match at least once.
    known_rust_extra_rows: list = field(default_factory=list)
    # Known DB cell divergences: [(table, fix_issue, match_dict, column,
    # dj_value, rs_value)]. Rows in `table` whose normalised cells all
    # equal `match_dict` have `column` blanked — but only when the cell
    # exactly equals the declared per-side value. Anything else (a side
    # changed, the fix landed) leaves the cell in place and fails the
    # compare loudly. Session-wide: state divergences persist across the
    # requests that follow the one that introduced them. Matchers and
    # values are templates over the gap ctx (world/static raw, captures
    # as ``{name}`` — the normalised form of substituted cells, so
    # capture placeholders match verbatim); a pin naming a capture not
    # seen yet is skipped for that request.
    known_cell_diffs: list = field(default_factory=list)

    def __post_init__(self):
        if not self.requests:
            raise ValueError(f"session {self.name}: no requests")


def render_template(node, context: dict[str, str]):
    """Render {placeholders} in str keys/values of a JSON-ish template."""
    if isinstance(node, str):
        try:
            return node.format(**context)
        except KeyError as exc:
            missing = exc.args[0]
            raise KeyError(
                f"unresolved placeholder {{{missing}}}: captured so far "
                f"{sorted(context)}"
            ) from exc
    if isinstance(node, dict):
        return {render_template(k, context): render_template(v, context) for k, v in node.items()}
    if isinstance(node, list):
        return [render_template(item, context) for item in node]
    if isinstance(node, tuple):
        return tuple(render_template(item, context) for item in node)
    return node


def extract_pointer(body, pointer: str):
    """Extract one value from a decoded JSON body via a/b/0/c pointer."""
    node = body
    for part in pointer.split("/"):
        if isinstance(node, list):
            try:
                node = node[int(part)]
            except (ValueError, IndexError):
                raise KeyError(f"pointer {pointer!r}: no index {part}")
        elif isinstance(node, dict):
            if part not in node:
                raise KeyError(f"pointer {pointer!r}: no key {part!r}")
            node = node[part]
        else:
            raise KeyError(f"pointer {pointer!r}: cannot descend into {type(node)}")
    return node
