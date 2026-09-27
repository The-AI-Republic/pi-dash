# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""LOGGING config regression tests (PDASHOSS01-208).

The local/production ``LOGGING`` dicts allow-list a handful of logger names.
Historically they had ``disable_existing_loggers: True`` and no root logger,
so every module-level ``logging.getLogger(__name__)`` logger (runner services,
managed_runner, cloud_agent, …) had no handler and effective level WARNING —
all of their structured events and ``logger.exception()`` calls were silently
dropped.

pytest's ``caplog`` attaches its own handler and bypasses the project LOGGING
config entirely, so these tests apply each settings module's dict with
``logging.config.dictConfig`` in a subprocess (importing local/production
settings in-process would mutate the shared INSTALLED_APPS/MIDDLEWARE lists)
and assert against the resulting logger tree and its real stderr output.
"""

import json
import os
import subprocess
import sys

import pytest

_PROBE = r"""
import importlib
import json
import logging
import logging.config
import sys

mod = importlib.import_module(sys.argv[1])
logging.config.dictConfig(mod.LOGGING)


def reachable_handlers(logger):
    handlers = []
    current = logger
    while current:
        handlers.extend(current.handlers)
        if not current.propagate:
            break
        current = current.parent
    return handlers


results = {}

# A representative module logger that is not in the explicit allow-list.
svc = logging.getLogger("pi_dash.runner.services.session_service")
results["module_info_enabled"] = svc.isEnabledFor(logging.INFO)
results["module_has_handler"] = bool(reachable_handlers(svc))

# The explicitly configured loggers keep working.
results["api_request_info_enabled"] = logging.getLogger("pi_dash.api.request").isEnabledFor(logging.INFO)

# Warnings/errors from outside pi_dash.* (django, third-party) reach root.
third_party = logging.getLogger("some_third_party.module")
results["root_warning_enabled"] = third_party.isEnabledFor(logging.WARNING)
results["root_has_handler"] = bool(logging.getLogger().handlers)

# Emit once on stderr so the parent can assert delivery and no duplication.
svc.info("managed_runner.engine_version probe_marker_pdashoss01_208")
print(json.dumps(results))
"""


def _run_probe(settings_module):
    env = os.environ.copy()
    # Neither settings module may be imported with a test DJANGO_SETTINGS_MODULE
    # pointing elsewhere — the probe imports the module directly and only reads
    # its LOGGING dict, so Django setup is not required.
    env.setdefault("WEB_URL", "http://localhost")
    env.setdefault("APP_BASE_URL", "http://localhost")
    return subprocess.run(
        [sys.executable, "-c", _PROBE, settings_module],
        capture_output=True,
        text=True,
        timeout=120,
        env=env,
    )


@pytest.mark.unit
@pytest.mark.parametrize(
    "settings_module",
    ["pi_dash.settings.local", "pi_dash.settings.production"],
)
def test_module_loggers_reach_a_handler(settings_module):
    proc = _run_probe(settings_module)
    assert proc.returncode == 0, f"probe failed: stdout={proc.stdout!r} stderr={proc.stderr!r}"
    results = json.loads(proc.stdout.strip().splitlines()[-1])

    # getLogger(__name__) modules emit at INFO and have a real handler.
    assert results["module_info_enabled"], "pi_dash.* module loggers must be INFO-enabled"
    assert results["module_has_handler"], "pi_dash.* module loggers must reach a handler"
    # The explicit allow-list is unchanged.
    assert results["api_request_info_enabled"]
    # Root backstop for non-pi_dash warnings/errors.
    assert results["root_warning_enabled"], "root logger must be WARNING-enabled"
    assert results["root_has_handler"], "root logger must have a handler"

    # The INFO record was actually delivered to the console handler (stderr),
    # exactly once (propagate=False on the catch-all prevents double emission).
    occurrences = proc.stderr.count("probe_marker_pdashoss01_208")
    assert occurrences == 1, f"expected exactly one emission, got {occurrences}: stderr={proc.stderr!r}"
