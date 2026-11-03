# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Ops image + entrypoints + runbook contract suite (F37-12).

Replays the F37-12 golden
(``rust-api/fixtures/ops/image/entrypoints.golden.json``) against the Rust
container layer: the 7 ``rust-api/bin/docker-entrypoint-*.sh`` scripts,
``rust-api/Dockerfile.api``, and the runbook section of
``rust-api/README.md``.

Static only: no database, no broker, no docker daemon. The image build
itself runs as a CI step (see
``.github/workflows/rust-api-contract-tests-ops-image.yml``), not here.
The one live check runs the built binary's ``--help`` for every
ops/serve/worker argv the scripts use, so a subcommand renamed by a
command sub-issue fails here.

Environment (the suite fails — never skips — without it):

* ``PIDASH_API_BIN`` (or ``RUST_BIN``) — the built binary (default:
  ``rust-api/target/debug/pidash-api`` next to this checkout).
"""

import json
import os
import re
import shutil
import subprocess
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent.parent
RUST_API = REPO_ROOT / "rust-api"
BIN_DIR = RUST_API / "bin"
GOLDEN_PATH = RUST_API / "fixtures" / "ops" / "image" / "entrypoints.golden.json"
DOCKERFILE = RUST_API / "Dockerfile.api"
README = RUST_API / "README.md"
PY_BIN = REPO_ROOT / "apps" / "api" / "bin"

SCRIPTS = [
    "api",
    "api-local",
    "worker",
    "worker-local",
    "beat",
    "beat-local",
    "migrator",
]

WAIT = "pidash-api ops wait_for_db"
WAIT_MIG = "pidash-api ops wait_for_migrations"
REGISTER = 'pidash-api ops instance register-instance "$MACHINE_SIGNATURE"'
CONFIGURE = "pidash-api ops instance configure-instance"
BUCKET = "pidash-api ops create_bucket"
CACHE = "pidash-api ops clear_cache"

# The exact boot-step argv per script, in order ("each step's argv asserted").
EXPECTED_COMMANDS = {
    "api": [
        WAIT,
        WAIT_MIG,
        REGISTER,
        CONFIGURE,
        BUCKET,
        CACHE,
        'exec pidash-api serve --bind 0.0.0.0:"${PORT:-8000}"',
    ],
    "api-local": [
        WAIT,
        WAIT_MIG,
        REGISTER,
        CONFIGURE,
        BUCKET,
        CACHE,
        "exec pidash-api serve --bind 0.0.0.0:8000",
    ],
    "worker": [
        WAIT,
        WAIT_MIG,
        'exec pidash-api worker --concurrency "$concurrency"',
    ],
    "worker-local": [WAIT, WAIT_MIG, "exec pidash-api worker"],
    "beat": [WAIT, WAIT_MIG, "pidash-api worker"],
    "beat-local": [WAIT, WAIT_MIG, "exec pidash-api worker"],
    "migrator": [WAIT, "python manage.py migrate $1"],
}

# Subcommand prefixes the scripts may invoke (binary name stripped).
# Every pidash-api line in every script must start with one of these,
# and each must answer --help with exit 0.
HELP_PREFIXES = [
    ("ops", "wait_for_db"),
    ("ops", "wait_for_migrations"),
    ("ops", "instance", "register-instance"),
    ("ops", "instance", "configure-instance"),
    ("ops", "create_bucket"),
    ("ops", "clear_cache"),
    ("serve",),
    ("worker",),
]

# Script-local variables (assigned by the scripts themselves, not read
# from the environment): the signature collectors (api :11-21),
# MACHINE_SIGNATURE (api export :21, read back at :24), and the worker
# concurrency math (:19-28). Everything else $-referenced is an env dep.
LOCAL_VARS = {
    "HOSTNAME",
    "MAC_ADDRESS",
    "CPU_INFO",
    "MEMORY_INFO",
    "DISK_INFO",
    "SIGNATURE",
    "MACHINE_SIGNATURE",
    "concurrency",
}

# Runbook-table variables consumed by the Rust binary rather than the
# scripts or Dockerfile, with the reader that proves each is real.
BINARY_VARS = {
    # DbConfig::ENV_VAR (crates/db/src/config/mod.rs)
    "DATABASE_URL",
    # registry key (crates/db/src/config/registry.rs); pool wiring unwired
    "DATABASE_READ_REPLICA_URL",
    # Keyring::from_env (crates/db/src/config/encryption.rs)
    "SECRET_KEY",
    # RedisHandle::from_settings (serve) + clear_cache backend
    "REDIS_URL",
    # AmqpConfig::from_env (crates/jobs/src/amqp.rs)
    "AMQP_URL",
    # S3Env::from_env (crates/services/src/ops/storage.rs)
    "AWS_REGION",
    "AWS_DEFAULT_REGION",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_S3_ENDPOINT_URL",
    "AWS_S3_BUCKET_NAME",
    # seed_dir() (crates/services/src/ops/seeds.rs), main.rs seed_data_dir
    "SEED_DIR",
    # RELEASES_URL_ENV (bin/pidash-api/src/ops/instance.rs)
    "PIDASH_RELEASES_URL",
}

RUNBOOK_HEADING = "## Ops image + runbook (D-37)"


def script_path(name):
    return BIN_DIR / f"docker-entrypoint-{name}.sh"


def read_script(name):
    return script_path(name).read_text()


def py_script(name):
    return (PY_BIN / f"docker-entrypoint-{name}.sh").read_text()


def code_lines(text):
    """Non-blank, non-comment lines (the executable content)."""
    return [
        line
        for line in text.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]


def command_lines(text):
    """Boot-step invocations: pidash-api / python lines, exec or plain."""
    return [
        line
        for line in code_lines(text)
        if re.match(r"^(exec\s+)?(pidash-api|python)\b", line)
    ]


def golden():
    return json.loads(GOLDEN_PATH.read_text())


def pidash_bin():
    override = os.environ.get("PIDASH_API_BIN") or os.environ.get("RUST_BIN")
    if override:
        candidate = Path(override)
    else:
        candidate = RUST_API / "target" / "debug" / "pidash-api"
    if not candidate.is_file():
        pytest.fail(
            f"pidash-api binary not found at {candidate} "
            "(build it: cargo build --bin pidash-api, or set PIDASH_API_BIN)"
        )
    return str(candidate)


def runbook_section():
    text = README.read_text()
    start = text.index(RUNBOOK_HEADING)
    rest = text[start:]
    end = rest.find("\n## ", 1)
    return rest[:end] if end != -1 else rest


def runbook_env_table():
    section = runbook_section()
    env_at = section.index("### Environment")
    names = []
    for line in section[env_at:].splitlines():
        match = re.match(r"\|\s*`([^`]+)`", line)
        if match:
            names.append(match.group(1))
    return names


# --------------------------------------------------------------------------
# golden sanity + boot-order replay
# --------------------------------------------------------------------------


def test_golden_covers_all_scripts():
    data = golden()
    assert data["_fixture"] == "F37-12"
    assert set(data["entrypoints"]) == set(SCRIPTS)
    assert data["dockerfile"]["expose"] == "EXPOSE 8000 (:56)"
    assert data["dockerfile"]["cmd"] == 'CMD ["./bin/docker-entrypoint-api.sh"] (:58)'
    assert "set -e" in data["preamble"]


@pytest.mark.parametrize("name", SCRIPTS)
def test_script_files_exist_and_executable(name):
    path = script_path(name)
    assert path.is_file(), f"{path} missing"
    assert os.access(path, os.X_OK), f"{path} not executable"


def test_no_extra_entrypoints():
    found = sorted(p.name for p in BIN_DIR.glob("docker-entrypoint-*.sh"))
    assert found == sorted(f"docker-entrypoint-{n}.sh" for n in SCRIPTS)


@pytest.mark.parametrize("name", SCRIPTS)
def test_preamble(name):
    lines = read_script(name).splitlines()
    assert lines[0] == "#!/bin/bash"
    assert lines[1] == "set -e"


@pytest.mark.parametrize("name", SCRIPTS)
def test_command_sequence(name):
    """Each boot step's argv, in order (F37-12 boot_order, mapped)."""
    assert command_lines(read_script(name)) == EXPECTED_COMMANDS[name]


def _check_evidence(name, text, kind, payload):
    code = "\n".join(code_lines(text))
    if kind == "cmd":
        assert payload in command_lines(text), f"{name}: missing `{payload}`"
    elif kind == "verbatim_py":
        py_text = py_script(name)
        assert payload in py_text, (
            f"test premise broke: {payload!r} not in Python {name}"
        )
        assert payload in text, f"{name}: Python span not ported: {payload!r}"
    elif kind == "commented":
        assert payload not in code, f"{name}: `{payload}` still in code lines"
        assert payload in text, f"{name}: dropped `{payload}` not documented"
    elif kind == "absent":
        assert payload not in code, f"{name}: `{payload}` must not be in code lines"
    else:  # pragma: no cover - mapping bug, not product behavior
        raise AssertionError(f"unknown evidence kind {kind}")


def _replay(name, mapping):
    """Walk the golden boot_order; every step must map to script evidence.

    mapping: list of (golden-substring, [(kind, payload), ...]). The
    substring must occur in the corresponding golden step (so the mapping
    itself is checked against the golden), and every golden step must
    have an entry (so no step is silently unaccounted).
    """
    steps = golden()["entrypoints"][name]["boot_order"]
    assert len(mapping) == len(steps), (
        f"{name}: mapping has {len(mapping)} entries for {len(steps)} golden steps"
    )
    text = read_script(name)
    for (matcher, evidences), step in zip(mapping, steps):
        assert matcher in step, (
            f"{name}: mapping premise broke: {matcher!r} not in {step!r}"
        )
        for kind, payload in evidences:
            _check_evidence(name, text, kind, payload)


def _signature_span():
    # Python api.sh:11-21 (the sha256 machine-signature block).
    return "\n".join(py_script("api").splitlines()[10:21])


def _worker_math():
    # Python worker.sh:19-28 (the min(nproc,8) concurrency block).
    return "\n".join(py_script("worker").splitlines()[18:28])


def test_replay_api():
    _replay(
        "api",
        [
            ("wait_for_db (:3)", [("cmd", WAIT)]),
            ("wait_for_migrations (:5)", [("cmd", WAIT_MIG)]),
            ("machine signature", [("verbatim_py", _signature_span())]),
            ('register_instance "$MACHINE_SIGNATURE"', [("cmd", REGISTER)]),
            ("configure_instance (:27)", [("cmd", CONFIGURE)]),
            ("create_bucket (:30)", [("cmd", BUCKET)]),
            ("clear_cache (:33)", [("cmd", CACHE)]),
            ("collectstatic --noinput", [("commented", "collectstatic")]),
            ("exec gunicorn", [("cmd", EXPECTED_COMMANDS["api"][-1])]),
        ],
    )


def test_replay_api_local():
    _replay(
        "api-local",
        [
            (
                "same wait/signature/register/configure/bucket/cache steps",
                [
                    ("cmd", WAIT),
                    ("cmd", WAIT_MIG),
                    ("verbatim_py", _signature_span()),
                    ("cmd", REGISTER),
                    ("cmd", CONFIGURE),
                    ("cmd", BUCKET),
                    ("cmd", CACHE),
                ],
            ),
            (
                "DJANGO_SETTINGS_MODULE",
                [
                    (
                        "verbatim_py",
                        'export DJANGO_SETTINGS_MODULE="${DJANGO_SETTINGS_MODULE:-pi_dash.settings.local}"',
                    )
                ],
            ),
            (
                "exec uvicorn",
                [
                    ("cmd", EXPECTED_COMMANDS["api-local"][-1]),
                    ("commented", "uvicorn"),
                    ("commented", "--reload"),
                ],
            ),
        ],
    )


def test_replay_worker():
    _replay(
        "worker",
        [
            ("wait_for_db (:4)", [("cmd", WAIT)]),
            ("wait_for_migrations (:6)", [("cmd", WAIT_MIG)]),
            ("concurrency resolution", [("verbatim_py", _worker_math())]),
            ("exec celery", [("cmd", EXPECTED_COMMANDS["worker"][-1])]),
        ],
    )


def test_replay_worker_local():
    _replay(
        "worker-local",
        [
            ("wait_for_db (:4)", [("cmd", WAIT)]),
            ("wait_for_migrations (:5)", [("cmd", WAIT_MIG)]),
            (
                "exec watchmedo auto-restart",
                [
                    ("cmd", EXPECTED_COMMANDS["worker-local"][-1]),
                    ("commented", "watchmedo"),
                ],
            ),
        ],
    )


def test_replay_beat():
    _replay(
        "beat",
        [
            ("wait_for_db (:4)", [("cmd", WAIT)]),
            ("wait_for_migrations (:6)", [("cmd", WAIT_MIG)]),
            ("celery -A pi_dash beat -l info (:8", [("cmd", "pidash-api worker")]),
        ],
    )


def test_replay_beat_local():
    _replay(
        "beat-local",
        [
            ("wait_for_db (:4)", [("cmd", WAIT)]),
            ("wait_for_migrations (:5)", [("cmd", WAIT_MIG)]),
            (
                "exec watchmedo auto-restart",
                [
                    ("cmd", EXPECTED_COMMANDS["beat-local"][-1]),
                    ("commented", "watchmedo"),
                ],
            ),
        ],
    )


def test_replay_migrator():
    _replay(
        "migrator",
        [
            ("wait_for_db $1 (:4)", [("cmd", WAIT), ("absent", "wait_for_db $1")]),
            ("migrate $1 (:6)", [("cmd", "python manage.py migrate $1")]),
        ],
    )
    assert "migrate stays Django" in read_script("migrator")


@pytest.mark.parametrize("name", ["api", "api-local"])
def test_stale_lines_ported_as_is(name):
    """F37-12 BUGS: the stale comment + mid-file shebang are kept."""
    py_lines = py_script(name).splitlines()
    assert py_lines[6] == "# Create the default bucket"
    assert py_lines[7] == "#!/bin/bash"
    text = read_script(name)
    assert text.count("#!/bin/bash") == 2
    assert text.count("# Create the default bucket") == 2


def test_worker_comment_provenance():
    key_lines = [
        "The default is min(nproc, 8): a ceiling, never a floor.",
        "``exec`` so celery is PID 1 of the container and receives SIGTERM directly:",
    ]
    py_text, text = py_script("worker"), read_script("worker")
    for line in key_lines:
        assert line in py_text, f"test premise broke: {line!r} not in Python worker"
        assert line in text, f"worker comment dropped: {line!r}"


def test_beat_has_no_exec():
    """Python beat.sh:8 runs celery without exec; the port keeps that."""
    assert "celery -A pi_dash beat -l info" in py_script("beat")
    assert not py_script("beat").splitlines()[7].startswith("exec")
    assert "exec pidash-api worker" not in read_script("beat")


@pytest.mark.parametrize("name", SCRIPTS)
def test_no_django_commands_outside_migrator(name):
    code = "\n".join(code_lines(read_script(name)))
    if name == "migrator":
        assert code.count("python manage.py") == 1  # migrate only
    else:
        assert "python manage.py" not in code
        assert "manage.py" not in code


@pytest.mark.parametrize("name", SCRIPTS)
def test_bash_syntax(name):
    if shutil.which("bash") is None:
        pytest.fail("bash is required to syntax-check the entrypoints")
    proc = subprocess.run(
        ["bash", "-n", str(script_path(name))],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, proc.stderr


# --------------------------------------------------------------------------
# ops command names must exist
# --------------------------------------------------------------------------


@pytest.mark.parametrize("name", SCRIPTS)
def test_script_argv_uses_known_prefixes(name):
    """Every pidash-api line resolves to a --help-checked prefix below."""
    for line in command_lines(read_script(name)):
        if line.startswith("python "):
            assert name == "migrator", f"{name}: python invocation outside migrator"
            continue
        argv = line.removeprefix("exec ").split()[1:]  # drop binary name
        assert any(argv[: len(prefix)] == list(prefix) for prefix in HELP_PREFIXES), (
            f"{name}: unchecked argv `{line}`"
        )


@pytest.mark.parametrize(
    "prefix", HELP_PREFIXES, ids=[" ".join(p) for p in HELP_PREFIXES]
)
def test_ops_name_exists(prefix):
    """The subcommand answers --help (fails if a command issue renamed it)."""
    proc = subprocess.run(
        [pidash_bin(), *prefix, "--help"],
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert proc.returncode == 0, proc.stderr


# --------------------------------------------------------------------------
# Dockerfile
# --------------------------------------------------------------------------


def dockerfile_text():
    return DOCKERFILE.read_text()


def test_dockerfile_multistage():
    stages = re.findall(r"^FROM (\S+)(?: AS (\S+))?", dockerfile_text(), re.M)
    assert stages == [("rust:bookworm", "builder"), ("debian:bookworm-slim", "")]


def test_dockerfile_contract():
    text = dockerfile_text()
    assert "WORKDIR /code" in text
    assert "EXPOSE 8000" in text
    assert 'CMD ["./bin/docker-entrypoint-api.sh"]' in text


def test_dockerfile_env_keys_preserved():
    keys = re.findall(r"^ENV (\w+)=", dockerfile_text(), re.M)
    assert keys == [
        "PYTHONDONTWRITEBYTECODE",
        "PYTHONUNBUFFERED",
        "PIP_DISABLE_PIP_VERSION_CHECK",
        "INSTANCE_CHANGELOG_URL",
    ]
    assert (
        "INSTANCE_CHANGELOG_URL=https://airepublic.com/pages/691ef037bcfe416a902e48cb55f59891/"
        in dockerfile_text()
    )


def test_dockerfile_copies():
    text = dockerfile_text()
    assert "COPY --from=builder /tmp/pidash-api /usr/local/bin/pidash-api" in text
    assert "COPY apps/api/package.json ./package.json" in text
    assert "COPY apps/api/pi_dash/seeds/data/ ./seeds/" in text
    assert "COPY rust-api/bin/docker-entrypoint-*.sh ./bin/" in text
    for line in text.splitlines():
        if line.startswith("COPY"):
            assert "pidash-api/" not in line.split("COPY", 1)[1].replace(
                "/tmp/pidash-api", ""
            ), f"Rust sources leak into the image: {line}"


def test_dockerfile_perms_and_logs():
    text = dockerfile_text()
    assert "RUN mkdir -p /code/pi_dash/logs" in text
    assert "RUN chmod +x ./bin/*" in text
    assert "RUN chmod -R 777 /code" in text


def test_dockerfile_minimal_runtime():
    text = dockerfile_text()
    assert "pip install" not in text
    assert "cargo build --release --bin pidash-api" in text
    for tool in ("bash", "ca-certificates", "hostname", "iproute2", "procps"):
        assert tool in text, f"entrypoint tool missing from runtime: {tool}"
    # The binary links libssl.so.3 (fernet -> openssl); without the
    # runtime package it cannot start in the slim image.
    assert "libssl3" in text


# --------------------------------------------------------------------------
# runbook cross-check
# --------------------------------------------------------------------------


def script_env_deps():
    """$-referenced vars across the scripts, minus script-locals."""
    deps = set()
    for name in SCRIPTS:
        text = read_script(name)
        for var in re.findall(r"\$\{([A-Za-z_]\w*)[^}]*\}", text):
            deps.add(var)
        for var in re.findall(r"\$([A-Za-z_]\w*)", text):
            deps.add(var)
    return deps - LOCAL_VARS


def test_runbook_lists_every_script_env_dep():
    table = runbook_env_table()
    assert table, "runbook env table parsed empty"
    for var in sorted(script_env_deps()):
        assert var in table, f"script env ${var} missing from the runbook table"


def test_runbook_lists_dockerfile_env_keys():
    table = runbook_env_table()
    keys = re.findall(r"^ENV (\w+)=", dockerfile_text(), re.M)
    for key in keys:
        assert key in table, f"Dockerfile ENV {key} missing from the runbook table"


def test_runbook_table_vars_are_real():
    sources = "\n".join(read_script(n) for n in SCRIPTS) + dockerfile_text()
    for var in runbook_env_table():
        assert re.search(rf"\b{re.escape(var)}\b", sources) or var in BINARY_VARS, (
            f"runbook table var {var} is consumed nowhere"
        )


def test_runbook_covers_boot_and_replica():
    section = runbook_section()
    for name in SCRIPTS:
        assert f"docker-entrypoint-{name}.sh" in section
    assert "docker build -f rust-api/Dockerfile.api" in section
    assert "DATABASE_READ_REPLICA_URL" in section
    assert "ENABLE_READ_REPLICA" in section
    assert "F37-11" in section
