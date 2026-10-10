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
    # no body. Raw-string bodies (non-JSON posts) use raw_body instead.
    json: object = None
    raw_body: bytes | None = None
    # Query params and extra headers (templates in values).
    params: dict = field(default_factory=dict)
    headers: dict = field(default_factory=dict)
    # Which credential to send: a key from Session.auth_users, "api_key"
    # (the seeded workspace token), or "anonymous".
    auth: str = "anonymous"
    # Captures: {name: pointer} into this response's decoded JSON body.
    capture: dict = field(default_factory=dict)
    # Expected Django status. The oracle is differential (Rust must equal
    # Django), but the author's expectation pins Django so a corpus bug
    # (bad seed, wrong payload) fails fast instead of locking in a 500.
    expect_status: int | None = None

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
    timestamp_columns: dict = field(default_factory=dict)
    generated_columns: dict = field(default_factory=dict)
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
