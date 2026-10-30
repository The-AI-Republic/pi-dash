# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Model fields specific to the runner app."""

from __future__ import annotations

from django.db import models
from django.db.models.expressions import Expression


class _ColumnDefault(Expression):
    """Compiles to the bare SQL ``DEFAULT`` keyword — the only value
    Postgres accepts for a generated column in an INSERT or UPDATE."""

    def as_sql(self, compiler, connection):
        return "DEFAULT", []


class JSONKeyBigIntegerField(models.BigIntegerField):
    """A read-only ``bigint`` column Postgres derives from one key of a JSON
    column: ``GENERATED ALWAYS AS ((<json_column> ->> '<key>')::bigint) STORED``.

    Stand-in for Django 5's ``GeneratedField`` on Django 4.2. It exists so a
    value that lives inside a JSON bag still has a real column — with real
    planner statistics — for aggregation (``Sum("total_tokens")``), while the
    JSON stays the single source of truth: the database, not application
    code, keeps the two in step.

    Writes are impossible by construction: ``pre_save`` always sends
    ``DEFAULT``, and ``QuerySet.update(<field>=...)`` is rejected by Postgres.
    Inserts read the computed value back via ``RETURNING``; after an update,
    reload the row (or read the JSON) to see the new value.
    """

    # Read the computed value back into the instance on INSERT.
    db_returning = True

    def __init__(self, *args, source: str, key: str, **kwargs):
        self.source = source
        self.key = key
        kwargs["null"] = True
        kwargs["blank"] = True
        kwargs["editable"] = False
        super().__init__(*args, **kwargs)

    def deconstruct(self):
        name, path, args, kwargs = super().deconstruct()
        for implied in ("null", "blank", "editable"):
            kwargs.pop(implied, None)
        kwargs["source"] = self.source
        kwargs["key"] = self.key
        return name, path, args, kwargs

    def db_type(self, connection):
        source = connection.ops.quote_name(self.source)
        return f"bigint GENERATED ALWAYS AS (({source} ->> '{self.key}')::bigint) STORED"

    def pre_save(self, model_instance, add):
        return _ColumnDefault()


class JSONKeyTextField(models.TextField):
    """A read-only ``text`` column Postgres derives from one key of a JSON
    column: ``GENERATED ALWAYS AS (COALESCE(<json_column> ->> '<key>', ''))
    STORED``.

    The text sibling of :class:`JSONKeyBigIntegerField`, for a value that
    lives inside a JSON bag but is still wanted as a grouping dimension —
    ``values("refusal_category").annotate(Count("id"))`` gets a real column
    with real planner statistics instead of an expression index whose
    selectivity the planner has to guess at.

    The ``COALESCE`` matters: the folded column it replaces was
    ``blank=True, default=""``, so a row without the key must read back as
    ``""`` and not ``None`` or the API contract changes shape.

    Writes are impossible by construction — see
    :class:`JSONKeyBigIntegerField`.
    """

    db_returning = True

    def __init__(self, *args, source: str, key: str, **kwargs):
        self.source = source
        self.key = key
        kwargs["null"] = True
        kwargs["blank"] = True
        kwargs["editable"] = False
        super().__init__(*args, **kwargs)

    def deconstruct(self):
        name, path, args, kwargs = super().deconstruct()
        for implied in ("null", "blank", "editable"):
            kwargs.pop(implied, None)
        kwargs["source"] = self.source
        kwargs["key"] = self.key
        return name, path, args, kwargs

    def db_type(self, connection):
        source = connection.ops.quote_name(self.source)
        return f"text GENERATED ALWAYS AS (COALESCE({source} ->> '{self.key}', '')) STORED"

    def pre_save(self, model_instance, add):
        return _ColumnDefault()
