# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PR B / M2 — insert the global ``review`` PromptTemplate.

Idempotent: if a workspace=NULL ``review`` row already exists this is
a no-op. The body is the polymorphic router prompt that picks the
review kind (CODE / DESIGN / DESIGN_THEN_CODE / GENERIC) at runtime
from the impl run's ``done_payload``. See
``.ai_design/create_review_state/design.md`` §5 / §8.
"""

from __future__ import annotations

from django.db import migrations


REVIEW_NAME = "review"


def insert_review_template(apps, schema_editor):
    PromptTemplate = apps.get_model("prompting", "PromptTemplate")
    exists = PromptTemplate.objects.filter(
        workspace__isnull=True, name=REVIEW_NAME
    ).exists()
    if exists:
        return
    # Import lazily so the migration framework's app-loading state
    # does not pull in unrelated module imports at definition time.
    from pi_dash.prompting.seed import REVIEW_TEMPLATE_BODY

    PromptTemplate.objects.create(
        workspace=None,
        name=REVIEW_NAME,
        body=REVIEW_TEMPLATE_BODY,
        is_active=True,
        version=1,
    )


def remove_review_template(apps, schema_editor):
    PromptTemplate = apps.get_model("prompting", "PromptTemplate")
    PromptTemplate.objects.filter(
        workspace__isnull=True, name=REVIEW_NAME
    ).delete()


class Migration(migrations.Migration):

    dependencies = [
        ("prompting", "0001_initial"),
    ]

    operations = [
        migrations.RunPython(
            insert_review_template,
            reverse_code=remove_review_template,
        ),
    ]
