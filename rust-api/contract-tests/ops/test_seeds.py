# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-37 seeds data + loader parity (PIDASHCONV-811, fixture F37-10).

Two halves, no servers, no database, no Django import:

- The 8 mirrored JSON files under
  ``rust-api/crates/services/src/ops/seeds_data/`` are byte-identical to
  the Python tree (``apps/api/pi_dash/seeds/data/``) and to the
  Ported-from (``01a93e17``) sha256 pins below.
- The loader expectations in F37-10 are replayed against the REAL
  ``read_seed_file``: the ``def`` (``workspace_seed_task.py:49-68``) is
  extracted from the Python source with ``ast`` and executed with a fake
  ``settings`` object, so this suite never imports Django yet fails if
  those lines drift. The Rust side
  (``crates/services/src/ops/seeds.rs``) asserts the same fixture file
  in its unit tests — one contract, both backends.
"""

import ast
import hashlib
import json
import logging
import os
import pathlib
import types

import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
PY_SEEDS = REPO_ROOT / "apps/api/pi_dash/seeds/data"
MIRRORS = REPO_ROOT / "rust-api/crates/services/src/ops/seeds_data"
FIXTURE = REPO_ROOT / "rust-api/fixtures/ops/seeds/loader.golden.json"
LOADER_SRC = REPO_ROOT / "apps/api/pi_dash/bgtasks/workspace_seed_task.py"

SEED_NAMES = [
    "projects",
    "states",
    "labels",
    "issues",
    "cycles",
    "modules",
    "pages",
    "views",
]

# sha256 of each seeds/data/<name>.json at Ported from 01a93e17
# (zero drift to rust-dev verified at implementation time; the gate owns
# drift from here). Pinned as constants so the check needs no git history.
PORTED_FROM_SHA256 = {
    "projects": "744066042fd18e01a2cd987a3a3dcd22a68314c39ad40e621f1f25fc86c411ab",
    "states": "3ce284bdfe1fca52b049a40f7823159bef806b6b2ec796944c10191db41d0446",
    "labels": "20d624ca2344ca696f29e7d2d7584024f5371ee8538ae239eb942763f3d2a3fd",
    "issues": "80540b790cd061cb293409c5fdbfcec0288353386d80b9d6cb25467bbedd5357",
    "cycles": "ea0942f0d194980ea0653f9625c2eef56d4fccc25742a3eab5c30624378d9b16",
    "modules": "01329147a17c7bc4a3f65be369a6cb7cf941080d77db11471f995cfb0611e117",
    "pages": "a30ec915ac5214ed0915d9ee0a8e85b6ddfa4c803dffd26ccccc68e78b6416f4",
    "views": "7bb357c3be94e693d271789feaa20c3c112d45032180188a6a1985a696113fc5",
}


@pytest.fixture(scope="module")
def golden():
    with open(FIXTURE, "r", encoding="utf-8") as fh:
        return json.load(fh)


def _load_read_seed_file(seed_dir):
    """Execute the real ``read_seed_file`` def (lines 49-68) standalone.

    Only the ``def`` node runs, with ``settings`` faked; the module-level
    Django/celery imports never execute, so Django is never imported.
    """
    tree = ast.parse(LOADER_SRC.read_text(encoding="utf-8"))
    fns = [
        n
        for n in tree.body
        if isinstance(n, ast.FunctionDef) and n.name == "read_seed_file"
    ]
    assert len(fns) == 1
    fn = fns[0]
    assert (fn.lineno, fn.end_lineno) == (49, 68), "loader lines drifted"
    assert [a.arg for a in fn.args.args] == ["filename"]
    assert not fn.decorator_list
    namespace = {
        "os": os,
        "json": json,
        "logger": logging.getLogger("pi_dash.worker"),
        "settings": types.SimpleNamespace(SEED_DIR=str(seed_dir)),
    }
    exec(  # noqa: S102 — executes the real loader def, not a replica
        compile(ast.Module(body=[fn], type_ignores=[]), str(LOADER_SRC), "exec"),
        namespace,
    )
    return namespace["read_seed_file"]


class _Capture(logging.Handler):
    def __init__(self):
        super().__init__()
        self.records = []

    def emit(self, record):
        self.records.append((record.levelname, record.getMessage()))


@pytest.fixture()
def worker_logs():
    logger = logging.getLogger("pi_dash.worker")
    capture = _Capture()
    logger.addHandler(capture)
    try:
        yield capture.records
    finally:
        logger.removeHandler(capture)


def _seed_dir_with(tmp_path, files):
    """Build a fake SEED_DIR: {tmp}/ with data/<name> -> bytes."""
    data = tmp_path / "data"
    data.mkdir()
    for name, content in files.items():
        (data / name).write_bytes(content)
    return tmp_path


# --- byte mirrors --------------------------------------------------------


@pytest.mark.parametrize("name", SEED_NAMES)
def test_mirror_byte_identical_to_python_tree(name):
    mirror = (MIRRORS / f"{name}.json").read_bytes()
    assert mirror == (PY_SEEDS / f"{name}.json").read_bytes()


@pytest.mark.parametrize("name", SEED_NAMES)
def test_mirror_matches_ported_from_pin(name):
    mirror = (MIRRORS / f"{name}.json").read_bytes()
    assert hashlib.sha256(mirror).hexdigest() == PORTED_FROM_SHA256[name]


def test_fixture_inventory_matches_python_tree(golden):
    assert golden["_fixture"] == "F37-10"
    for name in SEED_NAMES:
        rows = json.loads((PY_SEEDS / f"{name}.json").read_bytes())
        case = golden["data_inventory"][name]
        assert len(rows) == case["rows"]
        for row in rows:
            assert sorted(row.keys()) == sorted(case["required_keys"])
    branches = {b["when"]: b for b in golden["loader"]["branches"]}
    assert (
        "Seed file {filename} not found in {settings.SEED_DIR}/data"
        in branches["FileNotFoundError -> None"]["log"]
    )
    assert (
        "Error decoding JSON from {filename}"
        in branches["json.JSONDecodeError -> None"]["log"]
    )


# --- Python loader oracle -------------------------------------------------


@pytest.mark.parametrize("name", SEED_NAMES)
def test_python_happy_path_per_file(name, golden, worker_logs):
    read_seed_file = _load_read_seed_file(PY_SEEDS.parent)
    value = read_seed_file(f"{name}.json")
    assert value == json.loads((PY_SEEDS / f"{name}.json").read_bytes())
    case = golden["data_inventory"][name]
    assert len(value) == case["rows"]
    for row in value:
        assert sorted(row.keys()) == sorted(case["required_keys"])
    assert worker_logs == []


def test_python_missing_file(tmp_path, worker_logs):
    seed_dir = _seed_dir_with(tmp_path, {})
    read_seed_file = _load_read_seed_file(seed_dir)
    assert read_seed_file("nope.json") is None
    assert worker_logs == [
        ("ERROR", f"Seed file nope.json not found in {seed_dir}/data")
    ]


def test_python_corrupt_json(tmp_path, worker_logs):
    seed_dir = _seed_dir_with(tmp_path, {"bad.json": b"{nope"})
    read_seed_file = _load_read_seed_file(seed_dir)
    assert read_seed_file("bad.json") is None
    assert worker_logs == [("ERROR", "Error decoding JSON from bad.json")]


def test_python_seed_dir_override(tmp_path, worker_logs):
    seed_dir = _seed_dir_with(tmp_path, {"custom.json": b'[{"overridden": true}]'})
    read_seed_file = _load_read_seed_file(seed_dir)
    assert read_seed_file("custom.json") == [{"overridden": True}]
    assert worker_logs == []


def test_python_absolute_filename_resets_join(tmp_path, worker_logs):
    target = tmp_path / "abs.json"
    target.write_bytes(b"[1]")
    read_seed_file = _load_read_seed_file("/does/not/exist")
    assert read_seed_file(str(target)) == [1]
    assert worker_logs == []


def test_python_invalid_utf8_propagates(tmp_path, worker_logs):
    seed_dir = _seed_dir_with(tmp_path, {"bin.json": b"\xff\xfe{"})
    read_seed_file = _load_read_seed_file(seed_dir)
    with pytest.raises(UnicodeDecodeError):
        read_seed_file("bin.json")


def test_python_directory_propagates(tmp_path, worker_logs):
    seed_dir = _seed_dir_with(tmp_path, {})
    (seed_dir / "data" / "dir.json").mkdir()
    read_seed_file = _load_read_seed_file(seed_dir)
    with pytest.raises(IsADirectoryError):
        read_seed_file("dir.json")


def test_python_not_a_directory_propagates(tmp_path, worker_logs):
    (tmp_path / "data").write_bytes(b"x")
    read_seed_file = _load_read_seed_file(tmp_path)
    with pytest.raises(NotADirectoryError):
        read_seed_file("x.json")


def test_python_empty_seed_dir_reads_cwd_relative(tmp_path, monkeypatch, worker_logs):
    (tmp_path / "data").mkdir()
    (tmp_path / "data" / "x.json").write_bytes(b"[2]")
    monkeypatch.chdir(tmp_path)
    read_seed_file = _load_read_seed_file("")
    assert read_seed_file("x.json") == [2]
    assert worker_logs == []
