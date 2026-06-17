# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Guard: configuration in the settings modules must be read through
``pi_dash.config.get_config``, not ``os.environ`` directly. This keeps the
registry the single catalog of every config key. Intentional exceptions carry
a ``# noqa: config-env-read`` marker on the same line."""

import pathlib

import pytest

import pi_dash.config as config_pkg
import pi_dash.settings as settings_pkg

ALLOW_MARKER = "noqa: config-env-read"


def _settings_files():
    root = pathlib.Path(settings_pkg.__path__[0])
    # test.py legitimately seeds os.environ via setdefault for the test harness.
    return [p for p in root.glob("*.py") if p.name not in {"__init__.py", "test.py"}]


@pytest.mark.unit
def test_settings_modules_do_not_read_os_environ_directly():
    offenders = []
    for path in _settings_files():
        for lineno, line in enumerate(path.read_text().splitlines(), start=1):
            if "os.environ" in line and ALLOW_MARKER not in line:
                offenders.append(f"{path.name}:{lineno}: {line.strip()}")
    assert not offenders, (
        "Settings modules must read config via pi_dash.config.get_config, not "
        "os.environ. Offending lines:\n" + "\n".join(offenders)
    )


@pytest.mark.unit
def test_every_seeded_key_is_registered_as_db():
    """Every key configure_instance seeds into InstanceConfiguration must be
    registered as db-sourced in OSS. Otherwise the resolver routes it to env
    and silently ignores the seeded DB row (the IS_*_ENABLED regression)."""
    from pi_dash.config.registry import CONFIG
    from pi_dash.license.management.commands.configure_instance import DERIVED_FLAG_KEYS
    from pi_dash.utils.instance_config_variables import instance_config_variables

    seeded = [item["key"] for item in instance_config_variables] + list(DERIVED_FLAG_KEYS)
    bad = [key for key in seeded if CONFIG.get(key, {}).get("source") != "db"]
    assert not bad, f"seeded keys not registered as db-sourced: {sorted(bad)}"


@pytest.mark.unit
def test_registry_catalogs_every_key_settings_read():
    """Every key passed to get_config(...) in the settings modules must be
    registered, so the registry stays a complete catalog (and strict mode in
    CI never trips at boot)."""
    import re

    call = re.compile(r"get_config\(\s*[\"']([A-Z0-9_]+)[\"']")
    missing = set()
    for path in _settings_files():
        for key in call.findall(path.read_text()):
            if not config_pkg.is_registered(key):
                missing.add(f"{path.name}:{key}")
    assert not missing, f"get_config called with unregistered keys: {sorted(missing)}"
