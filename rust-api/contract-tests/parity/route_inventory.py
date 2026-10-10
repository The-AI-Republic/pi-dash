# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Route parity: Django URL conf vs the Rust router (PIDASHCONV-820).

Dumps Django's full resolver tree (every leaf under
``apps/api/pi_dash/urls.py`` with each view's allowed HTTP methods),
loads the Rust inventory from ``pidash-api routes --json``, normalises
path parameters on both sides, and diffs them. CI
(``.github/workflows/rust-api-parity.yml``) fails when the live diff
contains anything not classified in ``EXPECTED.md``.

This module is tooling, not a pytest suite (its name matches no
``test_*.py`` pattern): Django is imported lazily inside
:func:`dump_django_routes` only, so importing this module never touches
Django and collection stays clean.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys

# Methods tracked on both sides. TRACE is transport-level (proxies and
# servers special-case it) and is asserted by nothing; it is recorded in
# dumps as part of ANY but never diffed.
TRACKED_METHODS = ("GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS")

# Top-level Django include module -> Rust route group (urls.py mount order;
# the four `api/` includes are indistinguishable by path prefix, so the
# group comes from the include, mirroring RouteGroup).
GROUP_MOUNTS = {
    "pi_dash.app.urls": "app",
    "pi_dash.assistant.urls": "assistant",
    "pi_dash.loop.urls": "loop",
    "pi_dash.prompting.urls": "prompting",
    "pi_dash.space.urls": "space",
    "pi_dash.license.urls": "license",
    "pi_dash.runner.web_urls": "runner_web",
    "pi_dash.api.urls": "api_v1",
    "pi_dash.runner.urls": "runner",
    "pi_dash.authentication.urls": "auth",
    "pi_dash.web.urls": "web",
}

# Fallback prefix map for top-level leaves mounted outside the includes
# (the drf-spectacular schema trio when enabled); edge.rs PREFIX_TABLE
# order, longest match wins.
GROUP_PREFIXES = (
    ("api/v1/runner/", "runner"),
    ("api/v1/", "api_v1"),
    ("api/public/", "space"),
    ("api/instances/", "license"),
    ("api/runners/", "runner_web"),
    ("api/", "app"),
    ("auth/", "auth"),
    ("", "web"),
)

_DJANGO_CONVERTER_RE = re.compile(r"<(?:[^:<>]+:)?([^<>]+)>")
_REGEX_NAMED_GROUP_RE = re.compile(r"\(\?P<([^>]+)>[^)]*\)")


def normalize_django_path(route: str) -> str:
    """Normalise a Django route to ``{param}`` form.

    Regex named groups (``(?P<pk>[^/]+)``) become ``{pk}`` first (they
    contain angle brackets the converter pattern would corrupt), then
    ``path()`` converters (``<int:page>``, ``<slug>``) become ``{page}``
    / ``{slug}``. DRF router anchors (``^``/``$``) are stripped and
    escaped dots (``\\.``) unescaped. A leading slash is added (Django
    routes have none).
    """
    if not route.startswith("/"):
        route = "/" + route
    route = _REGEX_NAMED_GROUP_RE.sub(r"{\1}", route)
    route = _DJANGO_CONVERTER_RE.sub(r"{\1}", route)
    route = route.replace("^", "").replace("$", "").replace("\\.", ".")
    return route


def shape_key(path: str) -> str:
    """Match key for a normalised path: every ``{param}`` becomes ``{}``.

    Parameter names do not affect URL matching on either side (Django
    binds by name, axum by position), and the ports do not always keep
    Django's names (``{pk}`` vs ``{cycle_id}``), so the diff joins on
    shape while EXPECTED.md records both spellings.
    """
    return re.sub(r"\{[^}]*\}", "{}", path)


def normalize_rust_path(path: str) -> str:
    """Normalise an axum path to the same ``{param}`` form.

    Axum 0.8 renders captures as ``{name}`` and wildcards as
    ``{*name}``; the wildcard marker is dropped so ``/f/{*rest}``
    compares equal to Django's ``/f/<path:rest>/``.
    """
    return path.replace("{*", "{")


def group_for_path(path: str) -> str:
    """Map a normalised path to its Rust route group (longest prefix)."""
    stripped = path.lstrip("/")
    for prefix, group in GROUP_PREFIXES:
        if stripped.startswith(prefix):
            return group
    return "web"  # unreachable: "" matches everything


def dump_django_routes(api_dir: str, settings_module: str) -> dict:
    """Walk the Django resolver tree and return the route inventory.

    The return value is JSON-serialisable::

        {"meta": {...}, "routes": [{"path", "raw", "methods", "any",
        "view", "name", "group"}, ...]}

    ``methods`` is the sorted tracked-method list; ``any`` is true for
    plain function views, which receive every method. Runs Django setup
    in-process; the caller must use an interpreter with the Django app
    dependencies installed.
    """
    sys.path.insert(0, os.path.abspath(api_dir))
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", settings_module)
    import django  # noqa: E402  -- lazy: importing this module stays clean

    django.setup()
    from django.urls import URLPattern, URLResolver, get_resolver  # noqa: E402

    routes = []
    duplicates = 0

    def visit(patterns, prefix: str, group: str | None) -> None:
        nonlocal duplicates
        for entry in patterns:
            pattern = entry.pattern
            route = getattr(pattern, "_route", None)
            if route is None:  # RegexPattern
                route = pattern._regex
            if isinstance(entry, URLResolver):
                child_group = group
                if group is None:
                    mount = getattr(entry, "urlconf_name", None)
                    mount = getattr(mount, "__name__", mount)
                    if isinstance(mount, str):
                        child_group = GROUP_MOUNTS.get(mount)
                visit(entry.url_patterns, prefix + route, child_group)
                continue
            if not isinstance(entry, URLPattern):
                continue
            full = prefix + route
            callback = entry.callback
            methods, any_method, kind = _allowed_methods(callback)
            view = f"{callback.__module__}.{getattr(callback, '__name__', '?')}"
            path = normalize_django_path(full)
            row = {
                "path": path,
                "raw": full,
                "methods": sorted(methods),
                "any": any_method,
                "kind": kind,
                "view": view,
                "name": entry.name,
                "group": group or group_for_path(path),
            }
            if any(
                existing["path"] == row["path"]
                and existing["methods"] == row["methods"]
                and existing["view"] == row["view"]
                for existing in routes
            ):
                # Exact duplicate registration (DRF router api-root is
                # mounted twice); the first match always wins, so the
                # second row carries no behaviour.
                duplicates += 1
                continue
            routes.append(row)

    visit(get_resolver().url_patterns, "", None)
    routes.sort(key=lambda row: row["path"])
    from django.conf import settings  # noqa: E402

    meta = {
        "settings": settings_module,
        "debug": settings.DEBUG,
        "spectacular": getattr(settings, "ENABLE_DRF_SPECTACULAR", False),
        "django": django.get_version(),
        "count": len(routes),
        "exact_duplicate_rows_dropped": duplicates,
    }
    return {"meta": meta, "routes": routes}


def _allowed_methods(callback) -> tuple[set[str], bool, str]:
    """Best-effort allowed tracked methods for one view callback.

    ViewSets carry the per-route ``actions`` mapping; DRF/plain class
    views carry ``http_method_names`` plus per-method handlers; plain
    functions receive everything (``any=True``). DRF views serve
    OPTIONS via the metadata handler, so it is always added; HEAD is
    never added (DRF defines no ``head`` — an authenticated HEAD falls
    through to ``http_method_not_allowed``, verified in
    ``rest_framework/views.py``). Returns ``(methods, any, kind)`` with
    kind ``drf`` | ``cbv`` | ``function``: the kind decides what Django
    answers for a disallowed method (DRF: JSON 405 body; plain CBV:
    empty 405 + Allow, same status+body as axum's).
    """
    actions = getattr(callback, "actions", None)
    if actions:
        methods = {m.upper() for m in actions}
        methods.add("OPTIONS")
        return methods & set(TRACKED_METHODS), False, "drf"
    view_cls = getattr(callback, "cls", None) or getattr(callback, "view_class", None)
    if view_cls is not None and hasattr(view_cls, "http_method_names"):
        methods = {
            m.upper()
            for m in view_cls.http_method_names
            if callable(getattr(view_cls, m, None))
        }
        module = getattr(view_cls, "__module__", "")
        if module.startswith("rest_framework") or _is_drf_view(view_cls):
            methods.add("OPTIONS")
            return methods & set(TRACKED_METHODS), False, "drf"
        return methods & set(TRACKED_METHODS), False, "cbv"
    # Plain function view: Django delivers every method to it.
    return set(TRACKED_METHODS), True, "function"


def _is_drf_view(view_cls) -> bool:
    return any(
        base.__module__.startswith("rest_framework") for base in view_cls.__mro__
    )


def load_rust_routes(binary: str) -> dict:
    """Run ``pidash-api routes --json`` and return its parsed inventory."""
    proc = subprocess.run(
        [binary, "routes", "--json"],
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"{binary} routes --json failed ({proc.returncode}): {proc.stderr.strip()}"
        )
    return json.loads(proc.stdout)


# Django shape -> Rust shape for compound human identifiers captured as
# one Rust param (verified: dashless segments proxy, so Django answers
# its own 404 — the alias is behaviorally exact).
ALIASES = {
    "/api/v1/workspaces/{}/issues/{}-{}/": "/api/v1/workspaces/{}/issues/{}/",
    "/api/v1/workspaces/{}/work-items/{}-{}/": "/api/v1/workspaces/{}/work-items/{}/",
    "/api/workspaces/{}/work-items/{}-{}/": "/api/workspaces/{}/work-items/{}/",
}

# Rust-only paths and their reviewed justifications. A Rust route with
# no Django counterpart that is not listed here fails the check: new
# Rust-only surface needs a reviewed justification.
RUST_ONLY_JUSTIFICATIONS = {
    "/healthz": "foundation liveness probe; Django answers its 404 page here by design",
    "/api/schema": "slashless 301; served when v1-openapi flags on, Django only with ENABLE_DRF_SPECTACULAR=1 (off in contract env)",
    "/api/schema/": "served when v1-openapi flags on, Django only with ENABLE_DRF_SPECTACULAR=1 (off in contract env)",
    "/api/schema/redoc/": "served when v1-openapi flags on, Django only with ENABLE_DRF_SPECTACULAR=1 (off in contract env)",
    "/api/schema/swagger-ui/": "served when v1-openapi flags on, Django only with ENABLE_DRF_SPECTACULAR=1 (off in contract env)",
    "/api/v1/workspaces/{slug}/issues/{segment}/": "single-param capture of Django's compound {project_identifier}-{issue_identifier} (alias)",
    "/api/v1/workspaces/{slug}/work-items/{segment}/": "single-param capture of Django's compound {project_identifier}-{issue_identifier} (alias)",
    "/api/workspaces/{slug}/work-items/{tail}/": "single-param capture of Django's compound {project_identifier}-{issue_identifier} (alias)",
}


def diff_routes(django: dict, rust: dict) -> dict:
    """Diff the Django dump against the Rust inventory.

    Returns ``{"rows": [...], "rust_only": [...]}``. Each row:
    ``{"django", "rust", "group", "disposition", "detail", "gap"}`` with
    disposition ``OWNED`` | ``MISSING``. Gap IDs are rule-assigned (see
    ``GAP_RULES`` in the checker); ``G0`` means unclassified and must be
    resolved by hand before finalizing EXPECTED.md.
    """
    rust_by_shape: dict[str, dict] = {}
    for group_name, group in rust["groups"].items():
        for route in group["routes"]:
            shape = shape_key(normalize_rust_path(route["path"]))
            rust_by_shape.setdefault(shape, []).append((group_name, route))
    rows = []
    for dj in django["routes"]:
        dj_shape = shape_key(dj["path"])
        aliased = ALIASES.get(dj_shape)
        candidates = rust_by_shape.get(aliased or dj_shape, [])
        if not candidates:
            rows.append(
                {
                    "django": dj["path"],
                    "rust": "-",
                    "group": dj["group"],
                    "disposition": "MISSING",
                    "detail": "no-rust-route",
                    "gap": _gap_for_unmatched(dj),
                }
            )
            continue
        if len(candidates) > 1:
            raise RuntimeError(f"shape collision in Rust table: {dj_shape}")
        _group_name, rr = candidates[0]
        owned = set(rr["owned"])
        proxied = set(rr["proxied"])
        gaps: list[str] = []
        notes: list[str] = []
        for method in TRACKED_METHODS:
            allows = method in dj["methods"]
            is_owned = method in owned
            is_proxied = method in proxied
            if allows and (is_owned or is_proxied):
                continue
            if allows:
                gaps.append(f"{method}:allows-but-405")
            elif is_owned:
                gaps.append(f"{method}:serves-but-django-405")
            elif is_proxied:
                continue
            elif dj["kind"] == "drf":
                gaps.append(f"{method}:drf-405body-vs-axum-405")
            else:
                notes.append(method)
        detail = ";".join(gaps) if gaps else "ok"
        if notes and not gaps:
            detail = f"ok (cbv-405: {','.join(notes)})"
        elif notes:
            detail = f"{detail};cbv-405:{','.join(notes)}"
        gap = _gap_for_gaps(dj, gaps)
        rows.append(
            {
                "django": dj["path"],
                "rust": rr["path"] if rr["path"] != dj["path"] else "=",
                "group": dj["group"],
                "disposition": "MISSING" if gaps else "OWNED",
                "detail": detail,
                "gap": gap,
            }
        )
    django_shapes = {shape_key(dj["path"]) for dj in django["routes"]}
    aliased_targets = set(ALIASES.values())
    rust_only = []
    for group_name, group in rust["groups"].items():
        for route in group["routes"]:
            shape = shape_key(normalize_rust_path(route["path"]))
            if shape in django_shapes or shape in aliased_targets:
                continue
            justification = RUST_ONLY_JUSTIFICATIONS.get(route["path"])
            rust_only.append(
                {
                    "rust": route["path"],
                    "group": group_name,
                    "justification": justification or "TODO-MANUAL",
                }
            )
    rust_only.sort(key=lambda row: row["rust"])
    return {"rows": rows, "rust_only": rust_only}


def _gap_for_unmatched(dj: dict) -> str:
    """Rule-assigned gap for a Django path with no Rust route."""
    path = dj["path"]
    if dj["group"] == "license":
        return "G2"
    if "{format}" in path:
        if "{pk}" in path:
            return "G5"
        return "G3"
    if path == "/api/v1/workspaces/{slug}/":
        return "G3"
    return "G0"


# Django paths whose disallowed methods are served by explicit
# `*_not_allowed` handlers that replay the view's auth + permission
# prelude and answer DRF's 405 bytes (read-verified; standing proof
# via contract assertions is the G6 fix issue).
NOT_ALLOWED_PATHS = {
    "/api/users/me/workspaces/",
    "/api/workspace-slug-check/",
    "/api/workspaces/",
    "/api/workspaces/{slug}/",
    "/api/workspaces/{slug}/user-activity/{user_id}/export/",
    "/api/workspaces/{slug}/workspace-themes/",
    "/api/workspaces/{slug}/workspace-themes/{pk}/",
}


def _gap_for_gaps(dj: dict, gaps: list[str]) -> str:
    """Rule-assigned gap for method-level differences (``-`` when clean).

    Each gap classifies separately: HEAD gaps are the axum auto-HEAD
    systematic (G1), except on the NOT_ALLOWED paths whose explicit
    `.head(not_allowed)` arms deny correctly (G6 with the rest);
    gaps on the magic-link routes are the missing proxy arms (G4);
    serves-denials on the NOT_ALLOWED paths are the read-verified
    handler denials pending contract proof (G6); anything else is
    unclassified (G0) and must be resolved by hand before finalizing
    EXPECTED.md.
    """
    if not gaps:
        return "-"
    ids: set[str] = set()
    for gap in gaps:
        method, _, reason = gap.partition(":")
        if (
            dj["path"] in NOT_ALLOWED_PATHS
            and reason == "serves-but-django-405"
        ):
            ids.add("G6")
        elif method == "HEAD":
            ids.add("G1")
        elif "/magic" in dj["path"]:
            ids.add("G4")
        else:
            ids.add("G0")
    return "+".join(sorted(ids))


def load_expected(path: str) -> set[str]:
    """Collect every ``expected-routes`` fenced block in EXPECTED.md."""
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    expected: set[str] = set()
    in_block = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped == "```expected-routes":
            in_block = True
            continue
        if in_block and stripped == "```":
            in_block = False
            continue
        if in_block and stripped:
            expected.add(stripped)
    return expected


def dump_django_fresh(api_dir: str, settings_module: str) -> dict:
    """Dump Django's URL conf in a fresh interpreter.

    ``registered``-style import state is order-dependent (importing the
    URL conf also imports task modules as a side effect), so the check
    never dumps in-process: a fresh ``dump-django`` subprocess gives the
    same inventory CI sees.
    """
    proc = subprocess.run(
        [
            sys.executable,
            os.path.abspath(__file__),
            "dump-django",
            "--api-dir",
            api_dir,
            "--settings",
            settings_module,
            "--out",
            "-",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"dump-django failed ({proc.returncode}): {proc.stderr.strip()}")
    return json.loads(proc.stdout)


def cmd_check(args: argparse.Namespace) -> int:
    """Diff live Django vs live Rust and fail on anything unlisted."""
    django = dump_django_fresh(args.api_dir, args.settings)
    rust = load_rust_routes(args.binary)
    diff = diff_routes(django, rust)
    expected = load_expected(args.expected)
    for line in sorted(expected):
        head = line.split(" :: ", 1)[0]
        if head in ("G0", "RUST-ONLY"):
            print(f"error: {args.expected} lists an unclassifiable row: {line}")
            return 1
    live = {
        f"{row['gap']} :: {row['django']} :: {row['detail']}"
        for row in diff["rows"]
        if row["disposition"] == "MISSING"
    }
    failures = 0
    for row in diff["rows"]:
        if row["disposition"] == "MISSING" and row["gap"] == "G0":
            print(f"unclassified (G0): {row['django']} :: {row['detail']}")
            failures += 1
    for line in sorted(live - expected):
        print(f"unlisted diff: {line}")
        failures += 1
    for line in sorted(expected - live):
        print(f"stale EXPECTED.md entry (fix landed? update the file): {line}")
        failures += 1
    unjustified = [row for row in diff["rust_only"] if row["justification"] == "TODO-MANUAL"]
    for row in unjustified:
        print(f"unjustified rust-only route: {row['rust']}")
        failures += 1
    owned = sum(1 for row in diff["rows"] if row["disposition"] == "OWNED")
    verdict = "all listed" if failures == 0 else f"{failures} problem(s)"
    print(
        f"routes: {len(diff['rows'])} django, {owned} owned, "
        f"{len(live)} missing, "
        f"{len(diff['rust_only'])} rust-only -- {verdict}"
    )
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Django/Rust route parity inventory")
    sub = parser.add_subparsers(dest="command", required=True)

    dump = sub.add_parser("dump-django", help="dump Django's URL conf as JSON")
    dump.add_argument("--api-dir", default="apps/api")
    dump.add_argument("--settings", default="pi_dash.settings.test")
    dump.add_argument("--out", default="-")

    check = sub.add_parser("check", help="fail on any diff not listed in EXPECTED.md")
    check.add_argument("--binary", default="pidash-api")
    check.add_argument("--api-dir", default="apps/api")
    check.add_argument("--settings", default="pi_dash.settings.test")
    check.add_argument(
        "--expected",
        default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "EXPECTED.md"),
    )

    args = parser.parse_args(argv)
    if args.command == "dump-django":
        inventory = dump_django_routes(args.api_dir, args.settings)
        text = json.dumps(inventory, indent=2, sort_keys=False) + "\n"
        if args.out == "-":
            sys.stdout.write(text)
        else:
            with open(args.out, "w", encoding="utf-8") as handle:
                handle.write(text)
        return 0
    if args.command == "check":
        return cmd_check(args)
    raise AssertionError(f"unreachable command {args.command}")  # pragma: no cover


if __name__ == "__main__":
    raise SystemExit(main())
