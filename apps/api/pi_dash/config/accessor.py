# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The single entry point for reading configuration values.

All config reads should go through :func:`get_config` (or :func:`get_many`)
rather than touching ``os.environ`` or the ``InstanceConfiguration`` table
directly. The registry decides, per key, whether the value comes from the
environment or the database.

Two tiers, by necessity:

* ``"env"`` keys resolve at any time, including Django settings import.
* ``"db"`` keys require the app registry (ORM) to be ready, so they can only
  be read at runtime. Reading one too early raises :class:`ConfigError` loudly
  rather than failing in a confusing way — settings files must only read
  ``"env"`` keys.

This module must not import ``django.conf.settings`` or any model at import
time: settings modules import *this*, so a top-level Django import would be
circular. All Django access is lazy, inside the functions.
"""

import logging
import os

from .registry import CONFIG

logger = logging.getLogger("pi_dash.config")

# Sentinel so callers can pass ``default=None`` distinctly from "no override".
_UNSET = object()


class ConfigError(RuntimeError):
    """Raised for unregistered keys (in strict mode) or misuse of the tiers."""


def _strict() -> bool:
    """Whether to raise (vs. warn) on an unregistered key.

    Strict in tests and when DEBUG is on, so CI catches unregistered keys;
    lenient in prod so a forgotten key degrades gracefully to an env read.
    Reads the environment directly to avoid importing Django settings here.
    """
    if os.environ.get("DJANGO_SETTINGS_MODULE", "").endswith(".test"):
        return True
    return os.environ.get("DEBUG", "0") not in ("0", "", "false", "False")


def _instance_configuration_model():
    """Lazily import the model, mapping early access to a clear error."""
    try:
        from pi_dash.license.models import InstanceConfiguration
    except Exception as exc:  # AppRegistryNotReady, ImportError, ...
        raise ConfigError(
            "a db-sourced config key was read before the app registry was "
            "ready (e.g. during settings import). Settings modules must only "
            "read 'env'-sourced keys."
        ) from exc
    return InstanceConfiguration


def _decrypt(value):
    if not value:
        return value
    from pi_dash.license.utils.encryption import decrypt_data

    return decrypt_data(value)


def _read_env(key, entry, default=_UNSET):
    fallback = entry.get("default") if default is _UNSET else default
    return os.environ.get(key, fallback)


def _resolve_db_row(entry, row, default=_UNSET):
    if row is None:
        return entry.get("default") if default is _UNSET else default
    value = row["value"]
    if row["is_encrypted"]:
        return _decrypt(value)
    return value


def _handle_unregistered(key, default=_UNSET):
    msg = f"unregistered config key {key!r}"
    if _strict():
        raise ConfigError(f"{msg} — register it in pi_dash/config/registry.py")
    logger.warning("%s; defaulting to env", msg)
    return os.environ.get(key) if default is _UNSET else os.environ.get(key, default)


def get_config(key: str, default=_UNSET):
    """Return the configured value for ``key`` (a string, or its default).

    ``default`` optionally overrides the registry default for this call — used
    by settings modules that keep their inline fallbacks. It applies to both
    env reads (when the var is unset) and db reads (when no row exists).
    """
    entry = CONFIG.get(key)
    if entry is None:
        return _handle_unregistered(key, default)
    if entry["source"] == "env":
        return _read_env(key, entry, default)
    if entry["source"] == "db":
        model = _instance_configuration_model()
        row = model.objects.filter(key=key).values("value", "is_encrypted").first()
        return _resolve_db_row(entry, row, default)
    raise ConfigError(f"config key {key!r} has invalid source {entry['source']!r}")


def get_many(keys) -> dict:
    """Resolve several keys at once, batching the db reads into one query."""
    result: dict = {}
    db_entries: dict = {}
    for key in keys:
        entry = CONFIG.get(key)
        if entry is None:
            result[key] = _handle_unregistered(key)
        elif entry["source"] == "env":
            result[key] = _read_env(key, entry)
        elif entry["source"] == "db":
            db_entries[key] = entry
        else:
            raise ConfigError(f"config key {key!r} has invalid source {entry['source']!r}")

    if db_entries:
        model = _instance_configuration_model()
        rows = {
            r["key"]: r
            for r in model.objects.filter(key__in=list(db_entries)).values("key", "value", "is_encrypted")
        }
        for key, entry in db_entries.items():
            result[key] = _resolve_db_row(entry, rows.get(key))
    return result


def get_bool(key: str) -> bool:
    """Truthy iff the value is the string ``"1"`` (the project's convention)."""
    return get_config(key) == "1"


def get_int(key: str, default: int | None = None) -> int | None:
    value = get_config(key)
    try:
        return int(value)
    except (TypeError, ValueError):
        return default
