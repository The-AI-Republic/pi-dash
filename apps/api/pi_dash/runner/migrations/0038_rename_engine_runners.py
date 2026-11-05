# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Rename existing engine runners to the server-assigned scheme.

``.ai_design/built_engine_as_runner/design.md`` §5 and §13 ("Engine runners:
kept with the same ids, pods and work dirs. Renamed to the new scheme").

Every live engine runner (``provisioning = desktop_bundled``) gets
``builtin-<machine>-<workspace>-<project>``, with ``-2``, ``-3`` on a clash
across the machine's live runners or within the pod. The naming rules are a
frozen copy of :func:`pi_dash.runner.services.machine_identity.engine_runner_name`
as it was when this migration was written, so a later change to that function
cannot change what this migration does on a database that has not run it yet. It runs after the executor,
``assigned_runner`` backfill and pruning steps (``0035``) and the token
expiries (``0037``), so pruned rows are not renamed.

Deterministic: runners are visited per machine, oldest first, so the oldest
engine for a project keeps the bare name. Idempotent: each runner's target is
computed excluding itself, so a second pass assigns every runner the name it
already has and writes nothing. Not reversible in data (the reverse is a
no-op): the old ``desktop-desktop-<uuid>-<PROJECT>`` names are not needed by
anything, the daemon addresses runners by id.
"""

import hashlib
import re

from django.db import migrations

ENGINE = "desktop_bundled"

# --- Frozen copy of machine_identity's naming (do not import the live one) ---

_PREFIX = "builtin"
_NAME_MAX = 128
_HASH_LEN = 8
_SLUG_RE = re.compile(r"[^a-z0-9]+")


def _slug(value) -> str:
    return _SLUG_RE.sub("-", (value or "").lower()).strip("-")


def _machine_slug(machine) -> str:
    return _slug(machine.host_label) or _slug(machine.label) or str(machine.pk).split("-")[0]


def _base_name(machine, workspace, project) -> str:
    parts = [_PREFIX, _machine_slug(machine), _slug(workspace.slug), _slug(project.identifier)]
    base = "-".join(p for p in parts if p)
    if len(base) <= _NAME_MAX:
        return base
    digest = hashlib.sha256(str(project.pk).encode()).hexdigest()[:_HASH_LEN]
    return f"{base[: _NAME_MAX - _HASH_LEN - 1].rstrip('-')}-{digest}"


def _with_suffix(base: str, n: int) -> str:
    suffix = f"-{n}"
    return f"{base[: _NAME_MAX - len(suffix)].rstrip('-')}{suffix}"


def _engine_runner_name(Runner, machine, workspace, project, pod, exclude_runner_id) -> str:
    runners = Runner._base_manager
    machine_names = runners.filter(dev_machine_id=machine.pk, revoked_at__isnull=True).exclude(pk=exclude_runner_id)
    # ``runner_unique_name_per_pod`` covers every row in the pod, revoked too.
    pod_names = runners.filter(pod_id=pod.pk).exclude(pk=exclude_runner_id)
    taken = set(machine_names.values_list("name", flat=True)) | set(pod_names.values_list("name", flat=True))
    base = _base_name(machine, workspace, project)
    if base not in taken:
        return base
    n = 2
    while _with_suffix(base, n) in taken:
        n += 1
    return _with_suffix(base, n)


def rename_engine_runners(apps, schema_editor=None) -> int:
    """Rename live engine runners; returns how many rows changed."""
    Runner = apps.get_model("runner", "Runner")
    runners = (
        Runner._base_manager.filter(
            provisioning=ENGINE,
            revoked_at__isnull=True,
            dev_machine__isnull=False,
            pod__isnull=False,
        )
        .select_related("dev_machine", "workspace", "pod", "pod__project")
        .order_by("dev_machine_id", "created_at", "id")
    )
    renamed = 0
    for runner in runners.iterator():
        name = _engine_runner_name(
            Runner,
            runner.dev_machine,
            runner.workspace,
            runner.pod.project,
            runner.pod,
            exclude_runner_id=runner.pk,
        )
        if name != runner.name:
            Runner._base_manager.filter(pk=runner.pk).update(name=name)
            renamed += 1
    return renamed


def forwards(apps, schema_editor):
    rename_engine_runners(apps, schema_editor)


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0037_machine_token_expiry_data"),
    ]

    operations = [
        migrations.RunPython(forwards, migrations.RunPython.noop),
    ]
