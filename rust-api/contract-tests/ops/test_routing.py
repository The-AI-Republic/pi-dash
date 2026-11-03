# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-37 replica routing parity (PIDASHCONV-812, fixture F37-11).

Replays the read-replica decision table against the REAL Python defs,
extracted with ``ast`` and executed standalone with faked imports — no
servers, no database, no Django import (the test_seeds.py precedent):

- ``middleware/db_routing.py`` (``ReadReplicaRoutingMiddleware``) runs
  with dummy ``django.http`` names and the REAL ``request_scope``
  ``set``/``clear`` injected in place of ``pi_dash.utils.core``.
- ``utils/core/request_scope.py`` runs with the real ``asgiref.local``
  when importable, else a minimal ``Local`` fake.
- ``utils/core/dbrouters.py`` (``ReadReplicaRouter``) runs with dummy
  ``django.db.models`` names and the REAL ``should_use_read_replica``;
  its one relative import is rewritten to the injected namespace.
- ``utils/core/mixins/view.py`` is import-free and runs as is.

The Rust side (``crates/api/src/ops/routing.rs``) asserts the same
fixture file in its unit tests — one contract, both backends. A few
static pins below tie named Rust/Python lines to the fixture so the two
hand-encodings cannot drift apart silently.
"""

import ast
import contextlib
import json
import pathlib
import sys
import types

import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
FIXTURE = REPO_ROOT / "rust-api/fixtures/ops/routing/decision_table.golden.json"
MIDDLEWARE_SRC = REPO_ROOT / "apps/api/pi_dash/middleware/db_routing.py"
REQUEST_SCOPE_SRC = REPO_ROOT / "apps/api/pi_dash/utils/core/request_scope.py"
DBROUTERS_SRC = REPO_ROOT / "apps/api/pi_dash/utils/core/dbrouters.py"
MIXIN_SRC = REPO_ROOT / "apps/api/pi_dash/utils/core/mixins/view.py"
ROUTING_RS = REPO_ROOT / "rust-api/crates/api/src/ops/routing.rs"
POOL_RS = REPO_ROOT / "rust-api/crates/db/src/pool.rs"
BIN_MAIN_RS = REPO_ROOT / "rust-api/bin/pidash-api/src/main.rs"

# Pinned def spans (the gate owns drift; these fail closed if the sources
# move, so the suite never silently replays drifted lines).
SPANS = {
    "middleware": {
        "ReadReplicaRoutingMiddleware": (24, 164),
        "READ_ONLY_METHODS": 38,
        "__call__": (48, 68),
        "process_view": (70, 96),
        "_should_use_read_replica": (98, 113),
        "_get_use_replica_attribute": (115, 144),
        "process_exception": (146, 164),
    },
    "request_scope": {
        "set_use_read_replica": (28, 44),
        "should_use_read_replica": (47, 59),
        "clear_read_replica_context": (62, 76),
    },
    "dbrouters": {
        "ReadReplicaRouter": (21, 75),
        "db_for_read": (30, 44),
        "db_for_write": (46, 58),
        "allow_migrate": (60, 75),
    },
    "mixin": {
        "ReadReplicaControlMixin": (10, 24),
        "use_read_replica": 24,
    },
}


@contextlib.contextmanager
def _fake_modules(modules):
    """Install ``{dotted_name: module}`` fakes, then restore."""
    saved = {name: sys.modules.get(name, ...) for name in modules}
    for name, module in modules.items():
        sys.modules[name] = module
    try:
        yield
    finally:
        for name, module in saved.items():
            if module is ...:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = module


def _span_of(path, name):
    tree = ast.parse(path.read_text(encoding="utf-8"))
    for node in ast.walk(tree):
        if isinstance(
            node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)
        ) and node.name == name:
            return (node.lineno, node.end_lineno)
        if isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name) and target.id == name:
                    return node.lineno
        if (
            isinstance(node, ast.AnnAssign)
            and isinstance(node.target, ast.Name)
            and node.target.id == name
        ):
            return node.lineno
    raise AssertionError(f"{name} not found in {path}")


def _check_spans(path, expected):
    for name, span in expected.items():
        assert _span_of(path, name) == span, f"{path.name}::{name} drifted"


def _exec_source(path, namespace):
    code = compile(
        ast.parse(path.read_text(encoding="utf-8")), str(path), "exec"
    )
    exec(code, namespace)  # noqa: S102 — executes the real defs, not a replica
    return namespace


def _load_request_scope():
    """Exec the real ``request_scope.py`` (real asgiref if importable)."""
    _check_spans(REQUEST_SCOPE_SRC, SPANS["request_scope"])
    try:
        import asgiref.local  # noqa: F401
    except ImportError:
        fake_local = types.ModuleType("asgiref.local")
        fake_local.Local = type("Local", (), {})
        fake_pkg = types.ModuleType("asgiref")
        fake_pkg.local = fake_local
        with _fake_modules({"asgiref": fake_pkg, "asgiref.local": fake_local}):
            return _exec_source(REQUEST_SCOPE_SRC, {"__name__": "request_scope"})
    return _exec_source(REQUEST_SCOPE_SRC, {"__name__": "request_scope"})


def _load_middleware(scope):
    """Exec the real middleware with the REAL scope set/clear injected."""
    _check_spans(MIDDLEWARE_SRC, SPANS["middleware"])
    django_http = types.ModuleType("django.http")
    django_http.HttpRequest = type("HttpRequest", (), {})
    django_http.HttpResponse = type("HttpResponse", (), {})
    django = types.ModuleType("django")
    django.http = django_http
    core = types.ModuleType("pi_dash.utils.core")
    core.set_use_read_replica = scope["set_use_read_replica"]
    core.clear_read_replica_context = scope["clear_read_replica_context"]
    utils = types.ModuleType("pi_dash.utils")
    utils.core = core
    root = types.ModuleType("pi_dash")
    root.utils = utils
    fakes = {
        "django": django,
        "django.http": django_http,
        "pi_dash": root,
        "pi_dash.utils": utils,
        "pi_dash.utils.core": core,
    }
    with _fake_modules(fakes):
        return _exec_source(MIDDLEWARE_SRC, {"__name__": "db_routing"})


def _load_router(scope):
    """Exec the real router with the REAL scope should injected."""
    _check_spans(DBROUTERS_SRC, SPANS["dbrouters"])
    tree = ast.parse(DBROUTERS_SRC.read_text(encoding="utf-8"))
    # Rewrite the one relative import to the injected absolute namespace;
    # every other statement runs exactly as written.
    for node in tree.body:
        if isinstance(node, ast.ImportFrom) and node.module == "request_scope":
            node.module = "pi_dash.utils.core.request_scope"
            node.level = 0
    models = types.ModuleType("django.db.models")
    models.Model = type("Model", (), {})
    db = types.ModuleType("django.db")
    db.models = models
    django = types.ModuleType("django")
    django.db = db
    request_scope_mod = types.ModuleType("pi_dash.utils.core.request_scope")
    request_scope_mod.should_use_read_replica = scope["should_use_read_replica"]
    core = types.ModuleType("pi_dash.utils.core")
    core.request_scope = request_scope_mod
    utils = types.ModuleType("pi_dash.utils")
    utils.core = core
    root = types.ModuleType("pi_dash")
    root.utils = utils
    fakes = {
        "django": django,
        "django.db": db,
        "django.db.models": models,
        "pi_dash": root,
        "pi_dash.utils": utils,
        "pi_dash.utils.core": core,
        "pi_dash.utils.core.request_scope": request_scope_mod,
    }
    with _fake_modules(fakes):
        namespace = {"__name__": "dbrouters"}
        code = compile(tree, str(DBROUTERS_SRC), "exec")
        exec(code, namespace)  # noqa: S102 — real defs, rewritten import only
        return namespace


def _load_mixin():
    """Exec the real mixin module (import-free, runs as is)."""
    _check_spans(MIXIN_SRC, SPANS["mixin"])
    return _exec_source(MIXIN_SRC, {"__name__": "view_mixin"})


@pytest.fixture(scope="module")
def golden():
    with open(FIXTURE, "r", encoding="utf-8") as fh:
        return json.load(fh)


@pytest.fixture(scope="module")
def py():
    """The real defs: scope fns, middleware/router classes, mixin class."""
    scope = _load_request_scope()
    middleware = _load_middleware(scope)
    router = _load_router(scope)
    mixin = _load_mixin()
    return types.SimpleNamespace(
        set_use_read_replica=scope["set_use_read_replica"],
        should_use_read_replica=scope["should_use_read_replica"],
        clear_read_replica_context=scope["clear_read_replica_context"],
        middleware=middleware["ReadReplicaRoutingMiddleware"],
        router=router["ReadReplicaRouter"](),
        mixin=mixin["ReadReplicaControlMixin"],
    )


@pytest.fixture(autouse=True)
def _clean_scope(py):
    py.clear_read_replica_context()
    yield
    py.clear_read_replica_context()


def _request(method):
    return types.SimpleNamespace(method=method, path="/api/v1/issues/")


def _view_func(func_attr=..., view_class_attr=..., cls_attr=...):
    """A fake view func; ... means the level carries no attribute."""

    def view(request):
        raise AssertionError("never called")

    if func_attr is not ...:
        view.use_read_replica = func_attr
    if view_class_attr is not ...:
        view.view_class = types.SimpleNamespace(
            use_read_replica=view_class_attr
        )
    if cls_attr is not ...:
        view.cls = types.SimpleNamespace(use_read_replica=cls_attr)
    return view


def _model():
    return types.SimpleNamespace(_meta=types.SimpleNamespace(label="db.Issue"))


# --- fixture shape -------------------------------------------------------


def test_fixture_shape(golden):
    assert golden["_fixture"] == "F37-11"
    assert golden["bugs"] == []
    assert golden["middleware"]["read_only_methods"] == ["GET", "HEAD", "OPTIONS"]
    rows = golden["decision_table"]
    assert [row["replica"] for row in rows] == [False, False, True, False]
    assert "default" in golden["request_scope"]["should"].lower()
    assert "default" in golden["router"]["db_for_write"].lower()
    assert "default" in golden["router"]["allow_migrate"].lower()
    assert "True" in golden["mixin"]["default"]


def test_class_spans_pin_source_lines():
    # The loaders already assert these on every module load; this test
    # names the pin so a drift failure points here too.
    _check_spans(MIDDLEWARE_SRC, SPANS["middleware"])
    _check_spans(REQUEST_SCOPE_SRC, SPANS["request_scope"])
    _check_spans(DBROUTERS_SRC, SPANS["dbrouters"])
    _check_spans(MIXIN_SRC, SPANS["mixin"])


# --- middleware: __call__ ------------------------------------------------


def test_read_only_methods_match_fixture(py, golden):
    assert py.middleware.READ_ONLY_METHODS == set(
        golden["middleware"]["read_only_methods"]
    )
    assert py.middleware.READ_ONLY_METHODS == {"GET", "HEAD", "OPTIONS"}


@pytest.mark.parametrize("method", ["POST", "PUT", "PATCH", "DELETE", "get", "CUSTOM"])
def test_non_read_sets_primary_immediately(py, method):
    seen = {}

    def get_response(request):
        seen["replica"] = py.should_use_read_replica()
        return types.SimpleNamespace(status_code=200)

    middleware = py.middleware(get_response)
    response = middleware(_request(method))
    assert response.status_code == 200
    assert seen == {"replica": False}
    assert py.should_use_read_replica() is False


def test_read_defers_to_process_view(py):
    seen = {}

    def get_response(request):
        seen["replica"] = py.should_use_read_replica()
        return types.SimpleNamespace(status_code=200)

    middleware = py.middleware(get_response)
    middleware(_request("GET"))
    # __call__ sets nothing for reads; without process_view the view sees
    # the unset default (primary).
    assert seen == {"replica": False}
    assert py.should_use_read_replica() is False


@pytest.mark.parametrize("method", ["GET", "POST"])
def test_call_clears_on_exception(py, method):
    def get_response(request):
        raise ValueError("view blew up")

    middleware = py.middleware(get_response)
    with pytest.raises(ValueError, match="view blew up"):
        middleware(_request(method))
    assert py.should_use_read_replica() is False


# --- middleware: process_view matrix -------------------------------------


@pytest.mark.parametrize("method", ["GET", "HEAD", "OPTIONS"])
@pytest.mark.parametrize(
    "func_attr,view_class_attr,cls_attr,expected",
    [
        # Missing everywhere → primary (safe default).
        (..., ..., ..., False),
        # Explicit None everywhere → primary.
        (None, None, None, False),
        # Function level wins whatever it holds...
        (True, False, False, True),
        (False, True, True, False),
        (True, ..., ..., True),
        (False, ..., ..., False),
        # ...else view_class...
        (..., True, False, True),
        (..., False, True, False),
        (None, True, False, True),
        (None, False, True, False),
        # ...else cls.
        (..., ..., True, True),
        (..., ..., False, False),
        (None, None, True, True),
        (None, None, False, False),
        # bool() coercion: falsy 0/'' → primary, truthy → replica.
        (0, ..., ..., False),
        ("", ..., ..., False),
        (1, ..., ..., True),
        ("x", ..., ..., True),
        (..., 0, True, False),
        (..., "", True, False),
        (..., 1, False, True),
    ],
)
def test_process_view_matrix(
    py, method, func_attr, view_class_attr, cls_attr, expected
):
    middleware = py.middleware(lambda request: None)
    view = _view_func(func_attr, view_class_attr, cls_attr)
    assert (
        middleware.process_view(_request(method), view, (), {}) is None
    )
    assert py.should_use_read_replica() is expected
    py.clear_read_replica_context()


def test_none_view_func_routes_primary(py):
    middleware = py.middleware(lambda request: None)
    assert (
        middleware.process_view(_request("GET"), None, (), {}) is None
    )
    assert py.should_use_read_replica() is False


@pytest.mark.parametrize("method", ["POST", "PUT", "PATCH", "DELETE"])
def test_process_view_ignores_writes(py, method):
    middleware = py.middleware(lambda request: None)
    assert (
        middleware.process_view(
            _request(method), _view_func(True, True, True), (), {}
        )
        is None
    )
    assert py.should_use_read_replica() is False


def test_process_exception_clears_and_returns_none(py):
    py.set_use_read_replica(True)
    middleware = py.middleware(lambda request: None)
    assert (
        middleware.process_exception(_request("GET"), ValueError("x")) is None
    )
    assert py.should_use_read_replica() is False


# --- request_scope -------------------------------------------------------


def test_scope_default_is_primary(py):
    assert py.should_use_read_replica() is False


@pytest.mark.parametrize(
    "value,expected",
    [(True, True), (False, False), (1, True), (0, False), ("x", True), ("", False)],
)
def test_scope_set_stores_bool(py, value, expected):
    py.set_use_read_replica(value)
    assert py.should_use_read_replica() is expected


def test_scope_clear_is_idempotent(py):
    py.clear_read_replica_context()
    py.set_use_read_replica(True)
    py.clear_read_replica_context()
    assert py.should_use_read_replica() is False
    py.clear_read_replica_context()
    assert py.should_use_read_replica() is False


# --- router + mixin ------------------------------------------------------


def test_router_read_follows_scope(py):
    assert py.router.db_for_read(_model()) == "default"
    py.set_use_read_replica(True)
    assert py.router.db_for_read(_model()) == "replica"
    py.set_use_read_replica(False)
    assert py.router.db_for_read(_model()) == "default"


def test_router_write_always_primary(py):
    py.set_use_read_replica(True)
    assert py.router.db_for_write(_model()) == "default"
    py.clear_read_replica_context()
    assert py.router.db_for_write(_model()) == "default"


@pytest.mark.parametrize(
    "db,expected",
    [("default", True), ("replica", False), ("other", False)],
)
def test_router_migrate_primary_only(py, db, expected):
    assert py.router.allow_migrate(db, "db") is expected


def test_mixin_default_is_true(py):
    assert py.mixin.use_read_replica is True


# --- cross-pins: the Rust mirror names the same rules --------------------


def test_rust_read_only_methods_pin(golden):
    pool = POOL_RS.read_text(encoding="utf-8")
    assert 'matches!(method, "GET" | "HEAD" | "OPTIONS")' in pool
    assert set(golden["middleware"]["read_only_methods"]) == {
        "GET",
        "HEAD",
        "OPTIONS",
    }


def test_rust_mirror_pins():
    routing = ROUTING_RS.read_text(encoding="utf-8")
    assert "CONTROL_MIXIN_USE_READ_REPLICA: bool = true" in routing
    # Both __call__ branches run the inner service in a scope (set +
    # finally-clear); process_exception's clear rides the same exit.
    assert "replica_scope(false, inner.call(req))" in routing
    assert "replica_scope(use_replica, inner.call(req))" in routing


def test_rust_migrations_run_on_primary():
    main = BIN_MAIN_RS.read_text(encoding="utf-8")
    assert "ensure_schema(pools.primary())" in main
    assert "ensure_schema(pools.replica()" not in main
    assert "ensure_schema(pool_for" not in main
