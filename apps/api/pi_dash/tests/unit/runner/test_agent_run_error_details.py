# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Failure storage end to end (PDASHOSS01-187): the runner's terminal frames
through the HTTP endpoints into ``AgentRun.error_details``, the generated
``refusal_category`` column that keeps declines queryable, the migration's
backfill, and the API responses that keep their three flat keys."""

from __future__ import annotations

import importlib
import json

import pytest
from django.db import DatabaseError, connection, transaction
from django.db.models import Count
from django.utils import timezone

from pi_dash.app.serializers.issue import IssueDetailSerializer
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    RefusalCategory,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.serializers import AgentRunSerializer
from pi_dash.runner.services import tokens
from pi_dash.runner.services.error_details import merge_error_details

_MIGRATION = importlib.import_module("pi_dash.runner.migrations.0029_agent_run_error_details")


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def enrolled_runner(db, create_user, workspace, pod):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="errR",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        refresh_token_generation=1,
        enrolled_at=timezone.now(),
    )


@pytest.fixture
def runner_token(enrolled_runner):
    return tokens.mint_access_token(
        runner_id=str(enrolled_runner.id),
        user_id=str(enrolled_runner.owner_id),
        workspace_id=str(enrolled_runner.workspace_id),
        rtg=1,
    ).raw


@pytest.fixture
def running_run(db, create_user, workspace, pod, enrolled_runner):
    return AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        runner=enrolled_runner,
        prompt="x",
        status=AgentRunStatus.RUNNING,
        assigned_at=timezone.now(),
        started_at=timezone.now(),
    )


def _post(api_client, runner_token, path, body):
    return api_client.post(path, body, format="json", HTTP_AUTHORIZATION=f"Bearer {runner_token}")


def _refused(workspace, create_user, pod, category):
    return AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.REFUSED,
        error_details={"code": "provider_refusal", "message": "declined", "refusal_category": category},
    )


# ---------------------------------------------------------------------------
# The folded bag
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_clean_run_carries_an_empty_bag_and_reads_back_as_empty_strings(db, create_user, workspace, pod):
    run = AgentRun.objects.create(workspace=workspace, created_by=create_user, pod=pod)
    assert run.error_details == {}
    # The three flat names are the API contract; an absent key is "" and never
    # None, exactly as the dropped columns' ``default=""`` produced.
    assert (run.error_code, run.error, run.refusal_category) == ("", "", "")


@pytest.mark.unit
def test_flat_names_are_read_only_views_onto_the_bag(db, create_user, workspace, pod):
    run = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        error_details={"code": "run_timeout", "message": "worker was lost"},
    )
    assert (run.error_code, run.error) == ("run_timeout", "worker was lost")
    with pytest.raises(AttributeError):
        run.error_code = "nope"


@pytest.mark.unit
def test_merge_drops_a_key_rather_than_storing_an_empty_string():
    bag = merge_error_details({}, error_code="boom", error="detail")
    assert bag == {"code": "boom", "message": "detail"}
    # A COMPLETED run clears the prior attempt's text; the key goes away
    # instead of becoming "" so the bag stays a record of what is set.
    assert merge_error_details(bag, error="") == {"code": "boom"}
    # An unnamed key is left alone.
    assert merge_error_details(bag, error="x")["code"] == "boom"


# ---------------------------------------------------------------------------
# Runner frame → stored row
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_fail_frame_lands_in_the_bag(db, api_client, runner_token, running_run):
    resp = _post(
        api_client,
        runner_token,
        f"/api/v1/runner/runs/{running_run.id}/fail/",
        {"reason": "agent_crash", "detail": "boom"},
    )
    assert resp.status_code == 200, resp.data

    running_run.refresh_from_db()
    assert running_run.status == AgentRunStatus.FAILED
    assert running_run.error == "boom"
    assert running_run.error_details["message"] == "boom"
    assert "refusal_category" not in running_run.error_details


@pytest.mark.unit
def test_refusal_frame_records_its_category_in_the_bag(db, api_client, runner_token, running_run):
    resp = _post(
        api_client,
        runner_token,
        f"/api/v1/runner/runs/{running_run.id}/fail/",
        {"reason": "refusal", "detail": "declined under cyber policy", "category": "cyber"},
    )
    assert resp.status_code == 200, resp.data

    running_run.refresh_from_db()
    assert running_run.status == AgentRunStatus.REFUSED
    assert running_run.error_details["refusal_category"] == RefusalCategory.CYBER
    # …and the generated column agrees, because Postgres derives it.
    assert running_run.refusal_category == RefusalCategory.CYBER
    assert running_run.error == "declined under cyber policy"


@pytest.mark.unit
def test_complete_frame_clears_the_message_but_keeps_the_bag_a_dict(db, api_client, runner_token, running_run):
    AgentRun.objects.filter(pk=running_run.pk).update(error_details={"message": "daemon shutdown requested"})

    resp = _post(
        api_client,
        runner_token,
        f"/api/v1/runner/runs/{running_run.id}/complete/",
        {"done_payload": {"summary": "done"}},
    )
    assert resp.status_code == 200, resp.data

    running_run.refresh_from_db()
    assert running_run.status == AgentRunStatus.COMPLETED
    assert running_run.error_details == {}
    assert running_run.error == ""


# ---------------------------------------------------------------------------
# refusal_category stays a real column
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_refusal_category_is_a_stored_generated_column(db):
    with connection.cursor() as cursor:
        cursor.execute(
            """
            SELECT is_generated, data_type
            FROM information_schema.columns
            WHERE table_name = 'agent_run' AND column_name = 'refusal_category'
            """
        )
        assert cursor.fetchone() == ("ALWAYS", "text")


@pytest.mark.unit
def test_refusal_category_cannot_be_written_directly(db, create_user, workspace, pod):
    run = AgentRun.objects.create(workspace=workspace, created_by=create_user, pod=pod)
    with pytest.raises(DatabaseError), transaction.atomic():
        AgentRun.objects.filter(pk=run.pk).update(refusal_category="cyber")


@pytest.mark.unit
def test_declines_group_by_category_against_the_real_column(db, create_user, workspace, pod):
    """The question migration 0016 added the column for — "how many runs did
    the model decline last month, by category" — is still a plain column
    query, not a JSON expression the planner has to guess at."""
    _refused(workspace, create_user, pod, RefusalCategory.CYBER)
    _refused(workspace, create_user, pod, RefusalCategory.CYBER)
    _refused(workspace, create_user, pod, RefusalCategory.BIO)
    # A crash is not a decline and must not show up as a blank bucket.
    AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.FAILED,
        error_details={"code": "run_timeout", "message": "lost"},
    )

    qs = (
        AgentRun.objects.filter(status=AgentRunStatus.REFUSED)
        .values("refusal_category")
        .annotate(n=Count("id"))
        .order_by("refusal_category")
    )
    assert list(qs) == [
        {"refusal_category": RefusalCategory.BIO, "n": 1},
        {"refusal_category": RefusalCategory.CYBER, "n": 2},
    ]

    # Filtering reads the column itself — no ``->>`` in the generated SQL.
    filtered = AgentRun.objects.filter(refusal_category=RefusalCategory.CYBER)
    assert filtered.count() == 2
    sql = str(filtered.query)
    assert "refusal_category" in sql and "->>" not in sql

    # A non-refusal reads back as "" (the COALESCE), not NULL, so it never
    # becomes a surprise null bucket.
    assert AgentRun.objects.filter(refusal_category="").count() == 1


# ---------------------------------------------------------------------------
# The API contract stayed flat
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_run_serializer_keeps_the_three_flat_keys(db, create_user, workspace, pod):
    run = _refused(workspace, create_user, pod, RefusalCategory.CYBER)
    data = AgentRunSerializer(run).data
    assert data["error_code"] == "provider_refusal"
    assert data["error"] == "declined"
    assert data["refusal_category"] == RefusalCategory.CYBER
    # Derived at serialization time from ``error``; never stored, so it needed
    # no migration.
    assert data["error_diagnostic"]["summary"] == "declined"
    # And the bag itself stays behind the contract — this is a storage change.
    assert "error_details" not in data


@pytest.mark.unit
def test_clean_run_serializes_empty_strings_not_nulls(db, create_user, workspace, pod):
    run = AgentRun.objects.create(workspace=workspace, created_by=create_user, pod=pod, status=AgentRunStatus.COMPLETED)
    data = AgentRunSerializer(run).data
    assert (data["error"], data["error_code"], data["refusal_category"]) == ("", "", "")


@pytest.mark.unit
def test_issue_agent_status_panel_renders_the_same_keys(db, create_user, workspace, pod):
    run = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.FAILED,
        error_details={"code": "run_timeout", "message": "Cloud Agent worker was lost"},
    )
    composed = IssueDetailSerializer()._serialize_agent_run(run)
    assert composed["error_code"] == "run_timeout"
    assert composed["error"] == "Cloud Agent worker was lost"
    assert composed["error_diagnostic"]["summary"] == "Cloud Agent worker was lost"


# ---------------------------------------------------------------------------
# Rows written before the migration
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize(
    "legacy",
    [
        ("run_timeout", "Cloud Agent worker was lost", ""),
        ("provider_refusal", "declined under cyber policy", "cyber"),
        ("", "bare detail with no code", ""),
        ("desktop_not_connected", "", ""),
        ("", "", ""),
    ],
)
def test_migration_backfill_round_trips_legacy_rows(db, create_user, workspace, pod, legacy):
    """The migration's own SQL, run against a stand-in for the pre-0029 table.

    Only the table name is substituted, so this is the statement that runs in
    production. Forward proves a legacy row's three values land in the bag;
    reverse proves they come back out unchanged, which is what makes the
    migration safe to roll back.
    """
    table = "legacy_agent_run"
    forward = _MIGRATION._BACKFILL.replace("agent_run", table)
    backward = _MIGRATION._RESTORE.replace("agent_run", table)
    error_code, error, refusal_category = legacy

    with connection.cursor() as cursor:
        cursor.execute(
            f"""
            CREATE TEMP TABLE {table} (
                error_code varchar(64) NOT NULL DEFAULT '',
                error text NOT NULL DEFAULT '',
                refusal_category varchar(32) NOT NULL DEFAULT '',
                error_details jsonb NOT NULL DEFAULT '{{}}'::jsonb
            ) ON COMMIT DROP
            """
        )
        cursor.execute(
            f"INSERT INTO {table} (error_code, error, refusal_category) VALUES (%s, %s, %s)",
            legacy,
        )
        cursor.execute(forward)
        # ``::text`` because a temp table's jsonb comes back unparsed.
        cursor.execute(f"SELECT error_details::text FROM {table}")
        bag = json.loads(cursor.fetchone()[0])

        # Only the values that were actually set are present.
        expected = ("code", error_code), ("message", error), ("refusal_category", refusal_category)
        assert bag == {key: value for key, value in expected if value}

        cursor.execute(backward)
        cursor.execute(f"SELECT error_code, error, refusal_category FROM {table}")
        assert cursor.fetchone() == legacy

    # A run carrying the backfilled bag renders exactly what the three columns
    # rendered before the migration.
    migrated = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.FAILED,
        error_details=bag,
    )
    rendered = AgentRunSerializer(migrated).data
    assert (rendered["error_code"], rendered["error"], rendered["refusal_category"]) == legacy
