# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Rust replay for the export tasks (PIDASHCONV-1106).

Runs the same jobs as the Django oracle through the Rust worker
(``pidash-api worker`` serving ``rust_job_queue``; the Django worker
stays stopped) and requires identical effects: the same committed
captures (``exports/captures/``), the same ``exporters`` rows, and the
same mail. Any divergence from the oracle fails here.
"""

from __future__ import annotations

import io
import os
import zipfile

import pytest

from _harness import rust_queue
from _harness.db import wait_for_condition as wait_for
from _harness.seed import Seed

from . import cases

CAPTURES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "captures")


def _capture_tag(case: str) -> str:
    return f"cap-{case}"


@pytest.fixture()
def seed(db_conn):
    helper = Seed(db_conn)
    yield helper
    cases.cleanup_export_world(helper)
    helper.cleanup()


def _capture(name: str) -> bytes:
    with open(os.path.join(CAPTURES, name), "rb") as handle:
        return handle.read()


def _wait_exporter(token: str, db_conn, what: str) -> dict:
    def settled():
        row = cases.read_exporter(db_conn, token)
        # Terminal only: "processing" is transient on both backends.
        return row if row["status"] in ("completed", "failed") else None

    return wait_for(settled, what=what)


def _publish_issue_export(*, provider, world, token, multiple=False):
    return rust_queue.publish(
        cases.ISSUE_EXPORT_TASK,
        kwargs={
            "provider": provider,
            "workspace_id": world["workspace"],
            "project_ids": [world["project"]],
            "token_id": token,
            "multiple": multiple,
            "slug": world["slug"],
        },
    )


def test_rust_worker_liveness(db_conn):
    with db_conn.cursor() as cur:
        cur.execute("SELECT count(*) FROM rust_job_queue")
        (depth,) = cur.fetchone()
    assert depth >= 0


@pytest.mark.parametrize("provider", ["csv", "json"])
def test_rust_issue_export_matches_capture(seed, db_conn, provider):
    """Rust issue exports equal the committed Django bytes."""
    tag = _capture_tag(provider)
    world = cases.seed_export_world(seed, tag)
    token = f"cap-{tag}-{provider}"
    cases.seed_exporter(
        seed,
        workspace_id=world["workspace"],
        provider=provider,
        token=token,
        initiated_by_id=world["owner"],
    )
    celery_id = _publish_issue_export(provider=provider, world=world, token=token)
    row = _wait_exporter(token, db_conn, f"rust issue export {provider}")
    assert row["status"] == "completed", row
    assert row["reason"] == ""
    key = row["key"]
    assert key == cases.expected_s3_key(world, token), key
    rust_queue.wait_for_settled(celery_id, what=f"rust export {provider} settled")

    client = cases.s3_client()
    zipped = cases.get_object_bytes(client, key)
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        names = archive.namelist()
        assert len(names) == 1, names
        inner = archive.read(names[0])
    assert inner == _capture(f"issue_{provider}.golden"), "rust export bytes differ from Django"

    presigned = cases.parse_presigned(row["url"])
    assert presigned["expires"] == "604800"
    assert presigned["algorithm"] == "AWS4-HMAC-SHA256"
    assert presigned["signed_headers"] == "host"
    assert presigned["has_signature"]
    assert presigned["path"].endswith(f"/{key}"), presigned


def test_rust_issue_export_xlsx_cells(seed, db_conn):
    """Rust xlsx exports match the Django cells."""
    tag = _capture_tag("xlsx")
    world = cases.seed_export_world(seed, tag)
    token = f"cap-{tag}-xlsx"
    cases.seed_exporter(
        seed,
        workspace_id=world["workspace"],
        provider="xlsx",
        token=token,
        initiated_by_id=world["owner"],
    )
    celery_id = _publish_issue_export(provider="xlsx", world=world, token=token)
    row = _wait_exporter(token, db_conn, "rust issue export xlsx")
    assert row["status"] == "completed", row
    rust_queue.wait_for_settled(celery_id, what="rust xlsx settled")
    client = cases.s3_client()
    zipped = cases.get_object_bytes(client, row["key"])
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        inner = archive.read(archive.namelist()[0])
    import json as jsonlib

    payload = (jsonlib.dumps(cases.xlsx_cells(inner), indent=2, sort_keys=True) + "\n").encode(
        "utf-8"
    )
    assert payload == _capture("issue_xlsx.cells.json"), "rust xlsx cells differ from Django"


def test_rust_issue_export_bad_provider(seed, db_conn):
    """Rust marks unknown providers failed with the same reason."""
    world = cases.seed_export_world(seed, cases.unique_tag("bad"))
    token = cases.unique_tag("tok")
    cases.seed_exporter(
        seed,
        workspace_id=world["workspace"],
        provider="xml",
        token=token,
        initiated_by_id=world["owner"],
    )
    celery_id = _publish_issue_export(provider="xml", world=world, token=token)
    row = _wait_exporter(token, db_conn, "rust bad provider export")
    assert row["status"] == "failed", row
    assert row["reason"] == cases.BAD_PROVIDER_REASON, row
    assert row["url"] is None
    assert row["key"] == ""
    rust_queue.wait_for_settled(celery_id, what="rust bad provider settled")


def test_rust_issue_export_multiple(seed, db_conn):
    """Rust multiple mode zips one file per project id."""
    world = cases.seed_export_world(seed, cases.unique_tag("mul"))
    token = cases.unique_tag("tok")
    cases.seed_exporter(
        seed,
        workspace_id=world["workspace"],
        provider="csv",
        token=token,
        initiated_by_id=world["owner"],
    )
    celery_id = _publish_issue_export(provider="csv", world=world, token=token, multiple=True)
    row = _wait_exporter(token, db_conn, "rust multiple export")
    assert row["status"] == "completed", row
    rust_queue.wait_for_settled(celery_id, what="rust multiple settled")
    client = cases.s3_client()
    zipped = cases.get_object_bytes(client, row["key"])
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        names = archive.namelist()
    assert len(names) == 1, names
    assert names[0].endswith(".csv"), names


def _publish_analytic(*, email, data, slug):
    return rust_queue.publish(
        cases.ANALYTIC_EXPORT_TASK,
        kwargs={"email": email, "data": data, "slug": slug},
    )


def test_rust_analytic_export_flat_matches_capture(seed, db_conn, smtp_sink):
    """Rust flat plot mail equals the committed Django bytes."""
    tag = _capture_tag("flat")
    world = cases.seed_export_world(seed, tag)
    email = f"flat-{tag}@example.com"
    smtp_sink.clear()
    celery_id = _publish_analytic(
        email=email,
        data={"x_axis": "priority", "y_axis": "issue_count"},
        slug=world["slug"],
    )
    (delivered,) = smtp_sink.wait_for_count(1, what="rust analytic mail")
    assert delivered["subject"] == "Your Export is ready"
    assert delivered["rcpt_tos"] == [email]
    parsed = cases.parse_mail(delivered["raw"])
    assert len(parsed["attachments"]) == 1
    filename, content = parsed["attachments"][0]
    assert filename == f"{world['slug']}-analytics.csv"
    assert content == _capture("analytic_flat.csv.golden"), "rust CSV differs from Django"
    rust_queue.wait_for_settled(celery_id, what="rust analytic settled")


def test_rust_analytic_export_segmented_matches_capture(seed, db_conn, smtp_sink):
    """Rust segmented plot mail equals the committed Django bytes."""
    tag = _capture_tag("seg")
    world = cases.seed_export_world(seed, tag)
    email = f"seg-{tag}@example.com"
    smtp_sink.clear()
    celery_id = _publish_analytic(
        email=email,
        data={"x_axis": "priority", "y_axis": "issue_count", "segment": "state_id"},
        slug=world["slug"],
    )
    (delivered,) = smtp_sink.wait_for_count(1, what="rust segmented mail")
    parsed = cases.parse_mail(delivered["raw"])
    (filename, content), = parsed["attachments"]
    assert filename == f"{world['slug']}-analytics.csv"
    assert content == _capture("analytic_segmented.csv.golden"), "rust CSV differs from Django"
    rust_queue.wait_for_settled(celery_id, what="rust segmented settled")


def test_rust_analytic_export_longline_quoted_printable(seed, db_conn, smtp_sink):
    """Rust wide segmented CSV goes quoted-printable like Django (PIDASHCONV-1691)."""
    world = cases.seed_export_world(seed, cases.unique_tag("qp"))
    cases.seed_label_fanout(seed, world)
    email = f"qp-{world['slug']}@example.com"
    smtp_sink.clear()
    celery_id = _publish_analytic(
        email=email,
        data={"x_axis": "priority", "y_axis": "issue_count", "segment": "labels__id"},
        slug=world["slug"],
    )
    (delivered,) = smtp_sink.wait_for_count(1, what="rust long-line mail")
    assert delivered["subject"] == "Your Export is ready"
    assert delivered["rcpt_tos"] == [email]
    cases.assert_quoted_printable_csv_mail(delivered["raw"], world["slug"])
    rust_queue.wait_for_settled(celery_id, what="rust long-line settled")


def test_rust_analytic_export_estimate(seed, db_conn, smtp_sink):
    """Rust estimate plots mail the summed points."""
    world = cases.seed_export_world(seed, cases.unique_tag("est"))
    email = f"est-{world['slug']}@example.com"
    smtp_sink.clear()
    celery_id = _publish_analytic(
        email=email,
        data={"x_axis": "priority", "y_axis": "estimate"},
        slug=world["slug"],
    )
    (delivered,) = smtp_sink.wait_for_count(1, what="rust estimate mail")
    parsed = cases.parse_mail(delivered["raw"])
    (filename, content), = parsed["attachments"]
    text = content.decode("utf-8")
    assert '"Priority","Estimate"' in text.splitlines()[0]
    assert '"5.0"' in text, text
    rust_queue.wait_for_settled(celery_id, what="rust estimate settled")
