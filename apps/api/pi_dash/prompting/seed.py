# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Default template seed.

The global default `PromptTemplate` (workspace=NULL) is the runtime source of
truth, but the *initial* body is mirrored in
``apps/api/pi_dash/prompting/templates/default.j2`` so it can evolve in code
review. At migrate time we insert the row if it is missing. Operators who want
to re-sync the default after editing the file call the ``reseed_default_template``
management command — we never silently clobber a workspace row.
"""

from __future__ import annotations

import os
from pathlib import Path


DEFAULT_TEMPLATE_FILE = (
    Path(__file__).resolve().parent / "templates" / "default.j2"
)


def read_default_body() -> str:
    return DEFAULT_TEMPLATE_FILE.read_text(encoding="utf-8")


def seed_default_template(force: bool = False) -> str:
    """Create or (if ``force``) refresh the global default PromptTemplate.

    Returns one of: ``"created"``, ``"refreshed"``, ``"skipped"``.
    """
    from pi_dash.prompting.models import PromptTemplate

    body = read_default_body()
    existing = (
        PromptTemplate.objects.filter(
            workspace__isnull=True, name=PromptTemplate.DEFAULT_NAME
        )
        .order_by("-updated_at")
        .first()
    )
    if existing is None:
        PromptTemplate.objects.create(
            workspace=None,
            name=PromptTemplate.DEFAULT_NAME,
            body=body,
            is_active=True,
            version=1,
        )
        return "created"

    if force and existing.body != body:
        existing.body = body
        existing.version = (existing.version or 0) + 1
        existing.is_active = True
        existing.save(update_fields=["body", "version", "is_active", "updated_at"])
        return "refreshed"
    return "skipped"


def seed_default_template_on_migrate(
    sender=None, app_config=None, verbosity=1, using=None, **kwargs
) -> None:
    """`post_migrate` receiver. Only runs from the prompting app config."""
    # Running under unrelated apps is fine — post_migrate fires once per app —
    # but we gate on our own app to avoid creating multiple rows.
    if app_config is not None and app_config.label != "prompting":
        return
    if os.environ.get("PI_DASH_SKIP_PROMPT_SEED") == "1":
        return
    try:
        seed_default_template(force=False)
    except Exception as exc:  # noqa: BLE001
        # Seeding is best-effort during migrate; failures should not abort the
        # migrate command. Operators can re-run via management command.
        if verbosity:
            print(f"[prompting] default template seed skipped: {exc}")
