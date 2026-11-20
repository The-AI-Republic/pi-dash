# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Strip dead ``{{ current_site }}/static/seed_assets/`` image blocks (NEWPIDASH-308).

Onboarding issues seeded before commit 0bf942345 (2026-05-25) embed
``<image-component src="{{ current_site }}/static/seed_assets/NN.png">``
blocks. The placeholder was never templated and the ``seed_assets``
directory never existed, so the editor treats each ``src`` as an asset ID
and requests ``/api/assets/v2/.../%7B%7B%20current_site%20%7D%7D/...``
(which 502s). Commit 0bf942345 fixed new seeds only; existing rows are
still broken.

This is a data-only migration: no schema change, no model/serializer/view
change. It matches rows on the ``src`` substring only (no date or workspace
filter -- only seeded content can contain this literal) and, per row:

- removes every ``image-component`` element whose ``src`` contains the
  literal from ``description_html`` (all other content byte-identical);
- drops matching nodes from ``description_json`` (TipTap doc);
- recomputes ``description_stripped`` with the models' own ``strip_tags``
  (``pi_dash.utils.html_processor``, same as ``Issue.save``/``Page.save``);
- NULLs ``description_binary`` so the live server regenerates it from the
  cleaned HTML on next open (``apps/live/src/extensions/database.ts``
  ``fetchDocument`` converts empty-binary documents from HTML and saves
  the binary/HTML/JSON back).

Writes go through ``QuerySet.update()`` only -- ``save()`` is never called
and no signals fire, so no issue activity or notification noise is created.

Idempotent: a re-run matches only rows that still contain the literal, and
rows whose literal appears outside an ``image-component src`` are left
untouched, so a second run changes nothing.

Covers the ``Issue`` and ``Page`` tables (pre-fix page seeds were already
clean, but pages are handled defensively). Forward-only: the stripped
blocks are dead references, so the reverse is an explicit noop.
"""

import re

from django.db import migrations

from pi_dash.utils.html_processor import strip_tags

DEAD_SRC_SUBSTRING = "{{ current_site }}/static/seed_assets/"

# An <image-component ...> opening tag. Attribute values may be quoted, so
# `[^>]*` alone could stop early on a `>` inside quotes; the alternation
# consumes quoted values whole.
_IMAGE_COMPONENT_OPEN_TAG_RE = re.compile(
    r"<image-component\b(?:\"[^\"]*\"|'[^']*'|[^\"'>])*>",
    re.IGNORECASE,
)
_IMAGE_COMPONENT_CLOSE_TAG_RE = re.compile(r"</image-component\s*>", re.IGNORECASE)
_SRC_ATTR_RE = re.compile(
    r"""\bsrc\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)""",
    re.IGNORECASE,
)


def _opening_tag_has_dead_src(tag_text):
    match = _SRC_ATTR_RE.search(tag_text)
    if not match:
        return False
    value = match.group(1)
    if len(value) >= 2 and value[0] == value[-1] and value[0] in ("\"", "'"):
        value = value[1:-1]
    return DEAD_SRC_SUBSTRING in value


def strip_dead_seed_asset_blocks(html):
    """Remove dead image-component elements. Returns (cleaned_html, removed)."""
    if not html or DEAD_SRC_SUBSTRING not in html:
        return html, 0
    chunks = []
    pos = 0
    removed = 0
    for match in _IMAGE_COMPONENT_OPEN_TAG_RE.finditer(html):
        tag_text = match.group(0)
        if not _opening_tag_has_dead_src(tag_text):
            continue
        if tag_text.rstrip().endswith("/>"):
            end = match.end()
        else:
            close = _IMAGE_COMPONENT_CLOSE_TAG_RE.search(html, match.end())
            end = close.end() if close else match.end()
        chunks.append(html[pos : match.start()])
        pos = end
        removed += 1
    chunks.append(html[pos:])
    return "".join(chunks), removed


_DROP = object()


def _prune_json_value(value):
    """Recursively drop nodes whose attrs.src holds the literal.

    Returns (new_value, removed_count); dropped nodes yield (_DROP, n).
    """
    if isinstance(value, dict):
        attrs = value.get("attrs")
        if isinstance(attrs, dict) and DEAD_SRC_SUBSTRING in str(attrs.get("src", "")):
            return _DROP, 1
        changed = False
        removed = 0
        new_dict = {}
        for key, item in value.items():
            new_item, n = _prune_json_value(item)
            removed += n
            if new_item is _DROP:
                changed = True
                continue
            new_dict[key] = new_item
            if new_item is not item:
                changed = True
        return (new_dict if changed else value), removed
    if isinstance(value, list):
        changed = False
        removed = 0
        new_list = []
        for item in value:
            new_item, n = _prune_json_value(item)
            removed += n
            if new_item is _DROP:
                changed = True
                continue
            new_list.append(new_item)
            if new_item is not item:
                changed = True
        return (new_list if changed else value), removed
    return value, 0


def prune_dead_seed_asset_nodes(doc):
    """Drop dead-src nodes from a TipTap JSON doc. Returns (doc, removed)."""
    if not isinstance(doc, (dict, list)):
        return doc, 0
    new_doc, removed = _prune_json_value(doc)
    if new_doc is _DROP:
        return doc, 0
    return new_doc, removed


def strip_dead_seed_asset_images(apps, schema_editor):
    for model_name in ("Issue", "Page"):
        model = apps.get_model("db", model_name)
        matching_pks = list(
            model.objects.filter(
                description_html__contains=DEAD_SRC_SUBSTRING
            ).values_list("pk", flat=True)
        )
        for pk in matching_pks:
            obj = model.objects.only(
                "description_html", "description_json"
            ).get(pk=pk)
            cleaned_html, removed = strip_dead_seed_asset_blocks(
                obj.description_html or ""
            )
            if removed == 0:
                # Literal present outside an image-component src (e.g. plain
                # text): not an image block, leave the row untouched.
                continue
            pruned_json, _ = prune_dead_seed_asset_nodes(obj.description_json)
            stripped = strip_tags(cleaned_html) if cleaned_html else None
            model.objects.filter(pk=pk).update(
                description_html=cleaned_html,
                description_json=pruned_json,
                description_stripped=stripped,
                description_binary=None,
            )


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0182_project_settings"),
    ]

    operations = [
        migrations.RunPython(
            strip_dead_seed_asset_images,
            reverse_code=migrations.RunPython.noop,
        ),
    ]
