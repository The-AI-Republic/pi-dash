# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Cross-backend comparers for write-replay (PIDASHCONV-823).

Every comparer takes the two backends' observations plus a
:class:`CompareContext` (capture values, session tuning, sink bases)
and returns a human-readable error string, or ``None``/``[]`` when
equal. Anything not covered by an explicit rule in :mod:`replay.effects`
compares exactly.
"""

from __future__ import annotations

import json
import uuid as _uuid_mod
from dataclasses import dataclass, field
from datetime import date as _date_mod
from datetime import datetime, timezone
from decimal import Decimal as _Decimal_mod

from . import effects


@dataclass
class CompareContext:
    """Per-request comparison inputs."""

    # {capture_name: (django_value, rust_value)} accumulated so far.
    captures: dict = field(default_factory=dict)
    # Pointers captured from THIS response ({name: pointer}, body ones
    # only): values at these pointers are backend-local by declaration,
    # so both sides' values are replaced with {name} before comparing.
    capture_pointers: dict = field(default_factory=dict)
    # Session tuning (table -> [column]).
    timestamp_columns: dict = field(default_factory=dict)
    generated_columns: dict = field(default_factory=dict)
    # Extra skipped tables: {table: reason}.
    skip_tables: dict = field(default_factory=dict)
    # This backend run's webhook sink bases, for URL blanking.
    django_webhook_base: str = ""
    rust_webhook_base: str = ""
    # Request start (UTC) for the immediate-ETA window.
    request_start: datetime | None = None
    # Job payload timestamp pointers: {task_or_"*": [pointer, ...]}.
    payload_time_paths: dict = field(default_factory=dict)
    # Job payload row references: {task_or_"*": [(pointer, table), ...]}.
    payload_row_refs: dict = field(default_factory=dict)
    # DB cell row references: [(table, column, ref_table, pointer), ...].
    db_cell_refs: list = field(default_factory=list)
    # Existence check for row refs: fn(side, table, value) -> bool.
    # None leaves ref values untouched (they fail loudly in comparison).
    row_exists: object = None


# ---------------------------------------------------------------------------
# Frozen snapshots
# ---------------------------------------------------------------------------


def freeze(value):
    """Render a DB value to a stable comparable scalar (db._freeze rules)."""
    if isinstance(value, datetime):
        return value.isoformat()
    if isinstance(value, _date_mod):
        return value.isoformat()
    if isinstance(value, _uuid_mod.UUID):
        return str(value)
    if isinstance(value, memoryview):
        return value.tobytes().hex()
    if isinstance(value, bytes):
        return value.hex()
    if isinstance(value, _Decimal_mod):
        return str(value)
    return value


def snapshot_full(database_url: str) -> dict[str, list[dict]]:
    """Dump ``{table: [row, ...]}`` for every public table (frozen)."""
    import psycopg
    from psycopg.rows import dict_row

    from .snapshot import list_tables

    snap = {}
    with psycopg.connect(database_url) as conn, conn.cursor(row_factory=dict_row) as cur:
        for table in list_tables(database_url):
            cur.execute(f'SELECT * FROM "{table}"')
            snap[table] = [
                {col: freeze(val) for col, val in row.items()} for row in cur.fetchall()
            ]
    conn.close()
    return snap


# ---------------------------------------------------------------------------
# Database comparison
# ---------------------------------------------------------------------------


def _blank_webhook_base(value: str, base: str) -> str:
    if isinstance(value, str) and base and base in value:
        return value.replace(base, "<replay-sink>")
    return value


def _blank_db_cell_ref(cell, pointer: str, ref_table: str, side: str, ctx: CompareContext):
    """Existence-checked blanking for one DB cell (scalar or JSONB path)."""
    import copy

    if ctx.row_exists is None:
        return cell
    if not pointer:
        try:
            exists = ctx.row_exists(side, ref_table, cell)
        except Exception:
            exists = False
        return "{row:%s}" % ref_table if exists else cell
    if not isinstance(cell, dict):
        return cell
    cell = copy.deepcopy(cell)
    node = cell
    parts = pointer.split("/")
    try:
        for part in parts[:-1]:
            node = node[int(part)] if isinstance(node, list) else node[part]
        last = parts[-1]
        if isinstance(node, list):
            value = node[int(last)]
        elif isinstance(node, dict) and last in node:
            value = node[last]
        else:
            return cell
    except (KeyError, IndexError, ValueError, TypeError):
        return cell
    try:
        exists = ctx.row_exists(side, ref_table, value)
    except Exception:
        exists = False
    if not exists:
        return cell
    if isinstance(node, list):
        node[int(last)] = "{row:%s}" % ref_table
    else:
        node[last] = "{row:%s}" % ref_table
    return cell


def normalise_snapshot(snap: dict, side: str, ctx: CompareContext) -> dict:
    """Normalise one backend's full snapshot for cross-backend equality."""
    flat = effects.bind_captures(ctx.captures, side)
    base = ctx.django_webhook_base if side == "django" else ctx.rust_webhook_base
    skip = set(effects.SKIP_TABLES) | set(ctx.skip_tables)
    cell_refs: dict[tuple[str, str], list] = {}
    for entry in ctx.db_cell_refs:
        table, column, ref_table, pointer = entry
        cell_refs.setdefault((table, column), []).append((ref_table, pointer))
    out = {}
    for table, rows in snap.items():
        if table in skip:
            continue
        if table == effects.SESSIONS_TABLE:
            # Multiset of user_ids (capture-substituted), nothing else.
            users = sorted(
                flat.get(str(r.get("user_id")), str(r.get("user_id"))) for r in rows
            )
            out[table] = [{"user_id": u} for u in users]
            continue
        ts_extra = frozenset(ctx.timestamp_columns.get(table, ()))
        gen_extra = frozenset(ctx.generated_columns.get(table, ()))
        normed = []
        for row in rows:
            new = {}
            for col, val in row.items():
                cell = effects.normalise_db_value(table, col, val, ts_extra, gen_extra)
                if isinstance(cell, str):
                    if (table, col) in effects.WEBHOOK_URL_COLUMNS:
                        cell = _blank_webhook_base(cell, base)
                    if cell in flat:
                        cell = flat[cell]
                for ref_table, pointer in cell_refs.get((table, col), ()):
                    cell = _blank_db_cell_ref(cell, pointer, ref_table, side, ctx)
                new[col] = cell
            normed.append(new)
        normed.sort(key=lambda r: json.dumps(r, sort_keys=True, default=str))
        out[table] = normed
    return out


def _multiset_diff(left: list, right: list):
    """Rows in left-not-right and right-not-left (JSON-keyed multisets)."""
    from collections import Counter

    def key(row):
        return json.dumps(row, sort_keys=True, default=str)

    lc, rc = Counter(map(key, left)), Counter(map(key, right))
    only_left = [json.loads(k) for k, n in (lc - rc).items() for _ in range(n)]
    only_right = [json.loads(k) for k, n in (rc - lc).items() for _ in range(n)]
    return only_left, only_right


def compare_db(dj_norm: dict, rs_norm: dict) -> list[tuple[str, list, list]]:
    """Cross-backend normalised-state equality. Returns [(table, dj_only, rs_only)]."""
    diffs = []
    for table in sorted(set(dj_norm) | set(rs_norm)):
        if table not in dj_norm:
            diffs.append((table, [], rs_norm[table]))
        elif table not in rs_norm:
            diffs.append((table, dj_norm[table], []))
        else:
            only_dj, only_rs = _multiset_diff(dj_norm[table], rs_norm[table])
            if only_dj or only_rs:
                diffs.append((table, only_dj, only_rs))
    return diffs


def diff_states(before: dict, after: dict) -> dict[str, dict[str, list]]:
    """Changed rows per table between two normalised states (diagnostics)."""
    report = {}
    for table in sorted(set(before) | set(after)):
        removed, added = _multiset_diff(before.get(table, []), after.get(table, []))
        if added or removed:
            report[table] = {"added": added, "removed": removed}
    return report


# ---------------------------------------------------------------------------
# Response comparison
# ---------------------------------------------------------------------------


def _parse_set_cookie(headers) -> list[tuple[str, str]]:
    """Parse Set-Cookie headers to [(name, attributes)] with values blanked.

    Cookie *values* embed per-mint signatures/timestamps, so they can
    never match byte for byte; the names plus attributes (Path, HttpOnly,
    SameSite, ...) still compare. A wrong-user session still fails via
    the sessions-table user_id multiset.
    """
    out = []
    raw = headers.get("set-cookie")
    if raw is None:
        multi = getattr(headers, "get_list", None)
        values = multi("set-cookie") if multi else []
    else:
        values = [raw]
    for value in values:
        parts = [p.strip() for p in value.split(";")]
        if not parts or "=" not in parts[0]:
            continue
        name = parts[0].split("=", 1)[0]
        out.append((name, "; ".join(sorted(parts[1:]))))
    return sorted(out)


def _pointer_value(body, pointer: str):
    """Value at an a/b/0/c pointer, or None when missing."""
    node = body
    try:
        for part in pointer.split("/"):
            node = node[int(part)] if isinstance(node, list) else node[part]
    except (KeyError, IndexError, ValueError, TypeError):
        return None
    return node


def _blank_capture_pointers(body, pointers: dict):
    """Replace values at declared capture pointers with {name} (copy)."""
    import copy

    if not pointers:
        return body
    body = copy.deepcopy(body)
    for name, pointer in pointers.items():
        if pointer.startswith("@"):
            continue
        node = body
        parts = pointer.split("/")
        try:
            for part in parts[:-1]:
                node = node[int(part)] if isinstance(node, list) else node[part]
            last = parts[-1]
            if isinstance(node, list):
                node[int(last)] = "{%s}" % name
            elif isinstance(node, dict) and last in node:
                node[last] = "{%s}" % name
        except (KeyError, IndexError, ValueError, TypeError):
            continue
    return body


def _sub_capture_values(body, pointers: dict):
    """Substitute every occurrence of this response's captured values.

    A created id usually appears several times in its own response (the
    row plus its FK back-references); the declared pointers name the
    canonical occurrence, and every other occurrence of the same value
    is backend-local too.
    """
    flat = {}
    for name, pointer in pointers.items():
        if pointer.startswith("@"):
            continue
        value = _pointer_value(body, pointer)
        if isinstance(value, str) and value:
            flat[value] = "{%s}" % name
    if not flat:
        return body

    def walk(node):
        if isinstance(node, str):
            return flat.get(node, node)
        if isinstance(node, dict):
            return {k: walk(v) for k, v in node.items()}
        if isinstance(node, list):
            return [walk(v) for v in node]
        return node

    return walk(body)


def first_diffs(left, right, limit: int = 12, path: str = "$") -> list[str]:
    """Field-level differences between two normalised JSON values."""
    out: list[str] = []

    def walk(a, b, where: str) -> None:
        if len(out) >= limit:
            return
        if isinstance(a, dict) and isinstance(b, dict):
            for key in sorted(set(a) | set(b)):
                if key not in a:
                    out.append(f"{where}.{key}: missing on django, rust={b[key]!r:.160}")
                elif key not in b:
                    out.append(f"{where}.{key}: missing on rust, django={a[key]!r:.160}")
                else:
                    walk(a[key], b[key], f"{where}.{key}")
            return
        if isinstance(a, list) and isinstance(b, list):
            if len(a) != len(b):
                out.append(f"{where}: len {len(a)} vs {len(b)}")
            for index, (x, y) in enumerate(zip(a, b)):
                walk(x, y, f"{where}[{index}]")
            return
        if type(a) is not type(b) or a != b:
            out.append(f"{where}: django={a!r:.200} rust={b!r:.200}")

    walk(left, right, path)
    return out


def compare_response(
    dj_resp,
    rs_resp,
    ctx: CompareContext,
    known_headers: list | None = None,
    gap_ctx: dict | None = None,
) -> str | None:
    """Compare status, content-type, location/cookies, and body.

    ``known_headers`` ([(header, fix_issue, dj_value, rs_value)]) pins
    exact per-side header values (None = must be absent); pinned
    headers skip the standard checks below.
    """
    from .session import render_template

    if dj_resp.status_code != rs_resp.status_code:
        return (
            f"status differs: django={dj_resp.status_code} "
            f"rust={rs_resp.status_code}\n"
            f"django body: {dj_resp.text[:2000]}\n"
            f"rust body: {rs_resp.text[:2000]}"
        )
    pinned: set[str] = set()
    for header, issue, dj_want, rs_want in known_headers or []:
        name = header.lower()
        pinned.add(name)
        try:
            dj_want = (
                render_template(dj_want, gap_ctx or {}) if dj_want is not None else None
            )
            rs_want = (
                render_template(rs_want, gap_ctx or {}) if rs_want is not None else None
            )
        except KeyError as exc:
            return f"known-header pin {header} ({issue}): unrenderable match: {exc}"
        dj_have = dj_resp.headers.get(header)
        rs_have = rs_resp.headers.get(header)
        if dj_have != dj_want:
            return (
                f"known-header pin {header} ({issue}): Django header changed — "
                f"got {dj_have!r}, want {dj_want!r}"
            )
        if rs_have != rs_want:
            return (
                f"known-header pin {header} ({issue}): Rust header changed — "
                "fix landed or behavior changed; remove or update the pin. "
                f"got {rs_have!r}, want {rs_want!r}"
            )
    # Content-Type compares only when a representation exists: Django
    # sends no content-type on empty 204s while axum's JSON responder
    # keeps the header, and representation metadata on an empty body is
    # unobservable to clients (the contract suites pin status + empty
    # body for 204s, never the header).
    if "content-type" not in pinned and (dj_resp.content or rs_resp.content):
        dj_ct = dj_resp.headers.get("content-type", "")
        rs_ct = rs_resp.headers.get("content-type", "")
        if dj_ct != rs_ct:
            return f"content-type differs: django={dj_ct!r} rust={rs_ct!r}"
    if "location" not in pinned:
        dj_loc = dj_resp.headers.get("location")
        rs_loc = rs_resp.headers.get("location")
        if (dj_loc is None) != (rs_loc is None) or (
            dj_loc is not None and dj_loc != rs_loc
        ):
            dj_flat = effects.bind_captures(ctx.captures, "django")
            rs_flat = effects.bind_captures(ctx.captures, "rust")
            if dj_flat.get(dj_loc, dj_loc) != rs_flat.get(rs_loc, rs_loc):
                return f"location differs: django={dj_loc!r} rust={rs_loc!r}"
    if "set-cookie" not in pinned:
        dj_cookies = _parse_set_cookie(dj_resp.headers)
        rs_cookies = _parse_set_cookie(rs_resp.headers)
        if dj_cookies != rs_cookies:
            return f"set-cookie differs: django={dj_cookies!r} rust={rs_cookies!r}"
    if dj_resp.content == rs_resp.content:
        return None
    try:
        dj_json = dj_resp.json()
    except ValueError:
        dj_json = None
    try:
        rs_json = rs_resp.json()
    except ValueError:
        rs_json = None
    if dj_json is None or rs_json is None:
        return (
            "non-JSON body bytes differ:\n"
            f"django: {dj_resp.content[:2000]!r}\n"
            f"rust: {rs_resp.content[:2000]!r}"
        )
    dj_json = _sub_capture_values(dj_json, ctx.capture_pointers)
    rs_json = _sub_capture_values(rs_json, ctx.capture_pointers)
    dj_json = _blank_capture_pointers(dj_json, ctx.capture_pointers)
    rs_json = _blank_capture_pointers(rs_json, ctx.capture_pointers)
    dj_norm = effects.normalise_json_body(
        dj_json, effects.bind_captures(ctx.captures, "django")
    )
    rs_norm = effects.normalise_json_body(
        rs_json, effects.bind_captures(ctx.captures, "rust")
    )
    if dj_norm != rs_norm:
        lines = first_diffs(dj_norm, rs_norm)
        dj_text = json.dumps(dj_norm, sort_keys=True, indent=1, default=str)
        rs_text = json.dumps(rs_norm, sort_keys=True, indent=1, default=str)
        return (
            "JSON body differs (normalised):\n"
            + "\n".join(f"  {line}" for line in lines)
            + f"\ndjango: {dj_text[:2000]}\nrust: {rs_text[:2000]}"
        )
    return None


# ---------------------------------------------------------------------------
# Job comparison
# ---------------------------------------------------------------------------


def _parse_eta(value) -> datetime | None:
    if value is None:
        return None
    if isinstance(value, datetime):
        moment = value
    elif isinstance(value, str):
        try:
            moment = datetime.fromisoformat(value)
        except ValueError:
            return value  # unparseable: compares by equality below
    else:
        return value
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=timezone.utc)
    return moment


def eta_class(value, request_start: datetime | None):
    """Map an ETA/visible_at to None (immediate) or its datetime.

    Rust stamps visible_at=now() even with no delay while Django leaves
    eta null, so a stamp at-or-before the request start (+60s slack for
    clock granularity between the two requests) counts as immediate on
    both sides. Future stamps keep their datetime and compare within
    :data:`JOB_ETA_TOLERANCE_SECONDS`.
    """
    from datetime import timedelta

    moment = _parse_eta(value)
    if moment is None or not isinstance(moment, datetime):
        return moment
    if request_start is not None:
        start = request_start
        if start.tzinfo is None:
            start = start.replace(tzinfo=timezone.utc)
        if moment <= start + timedelta(seconds=60):
            return None
    return moment


def freeze_json(node):
    """Recursively freeze a job payload (tuples -> lists, UUIDs -> str)."""
    if isinstance(node, dict):
        return {k: freeze_json(v) for k, v in node.items()}
    if isinstance(node, (list, tuple)):
        return [freeze_json(v) for v in node]
    return freeze(node)


def _blank_nested_timestamps(node):
    """Blank TIMESTAMP_JSON_KEYS string values inside parsed nested JSON."""
    if isinstance(node, dict):
        return {
            key: (
                effects.VOLATILE_PLACEHOLDER
                if key in effects.TIMESTAMP_JSON_KEYS and isinstance(value, str)
                else _blank_nested_timestamps(value)
            )
            for key, value in node.items()
        }
    if isinstance(node, list):
        return [_blank_nested_timestamps(item) for item in node]
    return node


def _sub_payload(node, flat: dict):
    if isinstance(node, str):
        if node in flat:
            return flat[node]
        # Nested JSON strings (requested_data, updated_issue,
        # current_instance) compare semantically: queue consumers parse
        # them, so formatting is not behaviour — but the content (with
        # capture substitution and timestamp blanking) is.
        stripped = node.strip()
        if stripped.startswith(("{", "[")):
            try:
                parsed = json.loads(node)
            except ValueError:
                return node
            if isinstance(parsed, (dict, list)):
                return json.dumps(
                    _blank_nested_timestamps(_sub_payload(parsed, flat)),
                    sort_keys=True,
                    separators=(",", ":"),
                )
        return node
    if isinstance(node, dict):
        return {k: _sub_payload(v, flat) for k, v in node.items()}
    if isinstance(node, list):
        return [_sub_payload(v, flat) for v in node]
    return node


def _blank_time_paths(payload, paths: list[str], request_start: datetime | None):
    """Replace values at declared timestamp pointers with the placeholder.

    The placeholder applies only when the value is plausibly now (within
    10 minutes of the request start); a garbage timestamp (epoch 0, a
    far-future year) is left in place so it fails loudly.
    """
    import copy

    def _plausibly_now(value) -> bool:
        if request_start is None or isinstance(value, bool):
            return True
        try:
            if isinstance(value, (int, float)):
                moment = datetime.fromtimestamp(value, tz=timezone.utc)
            elif isinstance(value, str):
                try:
                    moment = datetime.fromtimestamp(float(value), tz=timezone.utc)
                except ValueError:
                    moment = datetime.fromisoformat(value)
                    if moment.tzinfo is None:
                        moment = moment.replace(tzinfo=timezone.utc)
            elif isinstance(value, datetime):
                moment = value
                if moment.tzinfo is None:
                    moment = moment.replace(tzinfo=timezone.utc)
            else:
                return False
        except (ValueError, OverflowError, OSError):
            return False
        start = request_start
        if start.tzinfo is None:
            start = start.replace(tzinfo=timezone.utc)
        return abs((moment - start).total_seconds()) <= 600

    payload = copy.deepcopy(payload)
    for pointer in paths:
        parts = pointer.split("/")
        node = payload
        try:
            for part in parts[:-1]:
                node = node[int(part)] if isinstance(node, list) else node[part]
            last = parts[-1]
            if isinstance(node, list):
                value = node[int(last)]
                if _plausibly_now(value):
                    node[int(last)] = effects.VOLATILE_PLACEHOLDER
            elif isinstance(node, dict) and last in node:
                if _plausibly_now(node[last]):
                    node[last] = effects.VOLATILE_PLACEHOLDER
        except (KeyError, IndexError, ValueError, TypeError):
            continue
    return payload


def _blank_row_refs(args, kwargs, task: str, side: str, ctx: CompareContext):
    """Replace existence-checked row ids with {row:<table>} placeholders.

    Pointers are multi-segment (``kwargs/a/0/b``) over dicts and lists
    and descend transparently through nested JSON strings (webhook
    payloads doubly-encode rows): each traversed string is re-serialized
    with the same canonical form :func:`_sub_payload` produced
    (sort_keys, compact separators), so untouched bytes round-trip
    exactly. A pointer that does not resolve is skipped: refs are
    session-level and a task's shape varies per request.
    """
    refs = list(ctx.payload_row_refs.get("*", ())) + list(
        ctx.payload_row_refs.get(task or "", ())
    )
    if not refs or ctx.row_exists is None:
        return args, kwargs
    import copy

    _MISSING = object()

    def _child(node, part):
        try:
            if isinstance(node, list):
                return node[int(part)]
            if isinstance(node, dict):
                return node.get(part, _MISSING)
        except (IndexError, ValueError, TypeError):
            pass
        return _MISSING

    def _set_child(node, part, value):
        if isinstance(node, list):
            node[int(part)] = value
        else:
            node[part] = value

    args, kwargs = copy.deepcopy(args), copy.deepcopy(kwargs)
    for pointer, table in refs:
        if pointer.startswith("args/"):
            root, rest = args, pointer[5:]
        elif pointer.startswith("kwargs/"):
            root, rest = kwargs, pointer[7:]
        else:
            continue
        parts = rest.split("/")
        node = root
        stack: list[tuple] = []
        resolved = True
        for part in parts[:-1]:
            child = _child(node, part)
            if child is _MISSING:
                resolved = False
                break
            if isinstance(child, str):
                try:
                    parsed = json.loads(child)
                except ValueError:
                    resolved = False
                    break
                if not isinstance(parsed, (dict, list)):
                    resolved = False
                    break
                stack.append((node, part, parsed))
                node = parsed
            else:
                node = child
        if not resolved:
            continue
        value = _child(node, parts[-1])
        if value is _MISSING:
            continue
        try:
            exists = ctx.row_exists(side, table, value)
        except Exception:
            exists = False
        if not exists:
            continue
        _set_child(node, parts[-1], "{row:%s}" % table)
        while stack:
            parent, part, parsed = stack.pop()
            _set_child(
                parent,
                part,
                json.dumps(parsed, sort_keys=True, separators=(",", ":")),
            )
    return args, kwargs


def normalise_celery_message(
    headers: dict, payload, side: str, ctx: CompareContext, queue: str = "celery"
) -> dict:
    """Normalise one observed Celery v2 message to (task, args, kwargs, eta)."""
    flat = effects.bind_captures(ctx.captures, side)
    args, kwargs = (list(payload[0]), dict(payload[1])) if len(payload) >= 2 else ([], {})
    args, kwargs = freeze_json(args), freeze_json(kwargs)
    task = headers.get("task")
    paths = list(ctx.payload_time_paths.get("*", ())) + list(
        ctx.payload_time_paths.get(task or "", ())
    )
    args = _blank_time_paths(
        args, [p[5:] for p in paths if p.startswith("args/")], ctx.request_start
    )
    kwargs = _blank_time_paths(
        kwargs, [p[7:] for p in paths if p.startswith("kwargs/")], ctx.request_start
    )
    args, kwargs = _sub_payload(args, flat), _sub_payload(kwargs, flat)
    if task == effects.PROCESS_LOGS_TASK:
        args, kwargs = _blank_topology_hosts(args), _blank_topology_hosts(kwargs)
    args, kwargs = _blank_row_refs(args, kwargs, task, side, ctx)
    return {
        "task": task,
        "args": args,
        "kwargs": kwargs,
        "eta": eta_class(headers.get("eta"), ctx.request_start),
        "retries": headers.get("retries", 0),
        "queue": queue,
    }


def normalise_queue_row(row: dict, ctx: CompareContext) -> dict:
    """Normalise one rust_job_queue row to the same shape."""
    flat = effects.bind_captures(ctx.captures, "rust")
    task = row.get("task")
    paths = list(ctx.payload_time_paths.get("*", ())) + list(
        ctx.payload_time_paths.get(task or "", ())
    )
    args = freeze_json(row.get("args", []))
    kwargs = freeze_json(row.get("kwargs", {}))
    args = _blank_time_paths(
        args, [p[5:] for p in paths if p.startswith("args/")], ctx.request_start
    )
    kwargs = _blank_time_paths(
        kwargs, [p[7:] for p in paths if p.startswith("kwargs/")], ctx.request_start
    )
    args, kwargs = _sub_payload(args, flat), _sub_payload(kwargs, flat)
    if task == effects.PROCESS_LOGS_TASK:
        args, kwargs = _blank_topology_hosts(args), _blank_topology_hosts(kwargs)
    args, kwargs = _blank_row_refs(args, kwargs, task, "rust", ctx)
    return {
        "task": task,
        "args": args,
        "kwargs": kwargs,
        "eta": eta_class(row.get("visible_at"), ctx.request_start),
        "retries": row.get("attempts", 0),
        "queue": row.get("queue", "celery"),
    }


def _blank_topology_hosts(node):
    """Blank 127.0.0.1:port values (process_logs Host header only)."""
    if isinstance(node, str):
        return effects.TOPOLOGY_HOST_RE.sub("<replay-host>", node)
    if isinstance(node, dict):
        return {k: _blank_topology_hosts(v) for k, v in node.items()}
    if isinstance(node, list):
        return [_blank_topology_hosts(v) for v in node]
    return node


def compare_jobs(
    dj_jobs: list[dict],
    rs_jobs: list[dict],
    known_absent: dict | None = None,
    known_shapes: list | None = None,
    gap_ctx: dict | None = None,
) -> str | None:
    """Compare normalised job multisets (order-insensitive, ETA-tolerant).

    ``known_absent`` ({task: fix_issue}) pins known divergences: each
    pinned task must be Django-present and Rust-absent, and is excluded
    from the multiset compare. ``known_shapes`` ([(task, fix_issue,
    dj_match, rs_match)]) pins exact normalised bytes per side for a
    task whose content diverges on this request: both sides must emit
    it and match byte for byte, else the pin fails loudly (fix landed
    or behavior changed — remove or update the pin).
    """
    from .session import render_template

    known_absent = known_absent or {}
    for task, issue in known_absent.items():
        dj_have = [j for j in dj_jobs if j["task"] == task]
        rs_have = [j for j in rs_jobs if j["task"] == task]
        if rs_have:
            return (
                f"known-divergence pin {task} ({issue}) is stale: Rust now "
                "emits it — remove the pin and compare fully"
            )
        if not dj_have:
            return (
                f"known-divergence pin {task} ({issue}): Django stopped "
                "emitting it — behavior changed, investigate"
            )
    dj_jobs = [j for j in dj_jobs if j["task"] not in known_absent]
    rs_jobs = [j for j in rs_jobs if j["task"] not in known_absent]
    for task, issue, dj_match, rs_match in known_shapes or []:
        dj_have = [j for j in dj_jobs if j["task"] == task]
        rs_have = [j for j in rs_jobs if j["task"] == task]
        if not dj_have or not rs_have:
            return (
                f"known-shape pin {task} ({issue}) never engaged "
                f"(django={len(dj_have)}, rust={len(rs_have)}) — the task "
                "stopped being emitted on this request or the pin is "
                "misplaced; remove or fix it"
            )
        try:
            dj_want = render_template(dj_match, gap_ctx or {})
            rs_want = render_template(rs_match, gap_ctx or {})
        except KeyError as exc:
            return f"known-shape pin {task} ({issue}): unrenderable match: {exc}"
        for job in dj_have:
            if job != dj_want:
                return (
                    f"known-shape pin {task} ({issue}): Django bytes changed — "
                    f"got {job!r:.400}"
                )
        for job in rs_have:
            if job != rs_want:
                return (
                    f"known-shape pin {task} ({issue}): Rust bytes changed — "
                    "fix landed or behavior changed; remove or update the pin. "
                    f"got {job!r:.400}"
                )
    pinned_tasks = {task for task, _i, _d, _r in (known_shapes or [])}
    dj_jobs = [j for j in dj_jobs if j["task"] not in pinned_tasks]
    rs_jobs = [j for j in rs_jobs if j["task"] not in pinned_tasks]
    remaining = list(rs_jobs)
    unmatched = []
    for job in dj_jobs:
        hit = None
        for cand in remaining:
            if (
                cand["task"] == job["task"]
                and cand["args"] == job["args"]
                and cand["kwargs"] == job["kwargs"]
                and cand.get("retries", 0) == job.get("retries", 0)
                and cand.get("queue", "celery") == job.get("queue", "celery")
            ):
                dj_eta, rs_eta = job.get("eta"), cand.get("eta")
                if dj_eta == rs_eta:
                    hit = cand
                    break
                if (
                    isinstance(dj_eta, datetime)
                    and isinstance(rs_eta, datetime)
                    and abs((dj_eta - rs_eta).total_seconds())
                    <= effects.JOB_ETA_TOLERANCE_SECONDS
                ):
                    hit = cand
                    break
        if hit is None:
            unmatched.append(job)
        else:
            remaining.remove(hit)
    if unmatched or remaining:
        return (
            "jobs enqueued differ:\n"
            f"django-only: {json.dumps(unmatched, default=str)[:3000]}\n"
            f"rust-only: {json.dumps(remaining, default=str)[:3000]}"
        )
    return None


# ---------------------------------------------------------------------------
# Sink comparison
# ---------------------------------------------------------------------------

# Delivery headers that legitimately differ per backend (client library,
# framing) and carry no behaviour. Everything else compares.
_DROPPED_MAIL_HEADERS = {"date", "message-id", "received", "dkim-signature"}
_DROPPED_HOOK_HEADERS = {
    "host",
    "content-length",
    "date",
    "user-agent",
    "accept",
    "accept-encoding",
    "connection",
}


def normalise_mail(message: dict, side: str, ctx: CompareContext) -> dict:
    from email import message_from_string

    flat = effects.bind_captures(ctx.captures, side)
    parsed = message_from_string(message.get("raw", ""))
    headers = {
        k.lower(): v
        for k, v in parsed.items()
        if k.lower() not in _DROPPED_MAIL_HEADERS
    }
    parts = []
    if parsed.is_multipart():
        for part in parsed.walk():
            if part.is_multipart():
                continue
            ctype = part.get_content_type()
            try:
                payload = part.get_content()
            except Exception:
                payload = part.get_payload(decode=True)
            parts.append((ctype, _sub_payload(str(payload), flat)))
    else:
        try:
            payload = parsed.get_content()
        except Exception:
            payload = parsed.get_payload()
        parts.append((parsed.get_content_type(), _sub_payload(str(payload), flat)))
    return {
        "from": message.get("mail_from"),
        "to": sorted(message.get("rcpt_tos") or []),
        "subject": _sub_payload(message.get("subject"), flat),
        "headers": headers,
        "parts": sorted(parts),
    }


def normalise_hook(delivery: dict, side: str, ctx: CompareContext) -> dict:
    flat = effects.bind_captures(ctx.captures, side)
    try:
        body = json.loads(delivery.get("body") or "")
    except ValueError:
        body = delivery.get("body")
    headers = {
        k.lower(): v
        for k, v in (delivery.get("headers") or {}).items()
        if k.lower() not in _DROPPED_HOOK_HEADERS
    }
    return {
        "path": delivery.get("path"),
        "headers": headers,
        "body": _sub_payload(body, flat),
    }


def compare_sinks(dj_mail, rs_mail, dj_hooks, rs_hooks, ctx) -> str | None:
    dj_m = sorted(
        (json.dumps(normalise_mail(m, "django", ctx), sort_keys=True, default=str) for m in dj_mail)
    )
    rs_m = sorted(
        (json.dumps(normalise_mail(m, "rust", ctx), sort_keys=True, default=str) for m in rs_mail)
    )
    dj_h = sorted(
        (json.dumps(normalise_hook(d, "django", ctx), sort_keys=True, default=str) for d in dj_hooks)
    )
    rs_h = sorted(
        (json.dumps(normalise_hook(d, "rust", ctx), sort_keys=True, default=str) for d in rs_hooks)
    )
    if dj_m != rs_m or dj_h != rs_h:
        return (
            "sink deliveries differ:\n"
            f"mail django={dj_m[:2]!r} rust={rs_m[:2]!r}\n"
            f"hooks django={dj_h[:2]!r} rust={rs_h[:2]!r}"
        )
    return None
