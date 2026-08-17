# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""M1 — insert the global ``test`` PromptTemplate.

Idempotent: if a workspace=NULL ``test`` row already exists this is a
no-op. The body is the polymorphic router prompt that picks the test
kind (AUTOMATED / UI / OPS / DESIGN / NON_TECHNICAL) at runtime from the
prior implementation run's ``done_payload``. See
``.ai_design/create_test_state/design.md`` §5 / §8.

Note: the ticking-phase runtime composes prompts from the section
registry (``prompting/sections/``) via recipes, not from this DB row —
this template mirrors the ``review`` precedent for parity and operator
visibility. The load-bearing content lives in the ``test-intro`` /
``test-cycle`` sections and the ``test`` recipe.
"""

from __future__ import annotations

from django.db import migrations


TEST_NAME = "test"


def insert_test_template(apps, schema_editor):
    PromptTemplate = apps.get_model("prompting", "PromptTemplate")
    exists = PromptTemplate.objects.filter(
        workspace__isnull=True, name=TEST_NAME
    ).exists()
    if exists:
        return
    # Import lazily so the migration framework's app-loading state
    # does not pull in unrelated module imports at definition time.
    from pi_dash.prompting.seed import TEST_TEMPLATE_BODY

    PromptTemplate.objects.create(
        workspace=None,
        name=TEST_NAME,
        body=TEST_TEMPLATE_BODY,
        is_active=True,
        version=1,
    )


def remove_test_template(apps, schema_editor):
    PromptTemplate = apps.get_model("prompting", "PromptTemplate")
    PromptTemplate.objects.filter(
        workspace__isnull=True, name=TEST_NAME
    ).delete()


class Migration(migrations.Migration):

    dependencies = [
        ("prompting", "0004_prompt_section_override"),
    ]

    operations = [
        migrations.RunPython(
            insert_test_template,
            reverse_code=remove_test_template,
        ),
    ]
