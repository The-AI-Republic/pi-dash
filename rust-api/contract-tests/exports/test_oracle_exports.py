# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Django oracle for the export tasks (PIDASHCONV-1106).

Publishes ``issue_export_task`` / ``analytic_export_task`` in Celery
wire format and lets the dedicated oracle worker run them (the stock
contract worker rejects both as unregistered — PDASHOSS01-292 — so the
session fixture boots Django with the modules force-included; see
``_harness/export_worker.py``). Captures the effects: exported file
bytes from S3, ``exporters`` rows, and mail from the SMTP sink.

The ``test_oracle_*_capture`` tests pin the effects byte-for-byte
against the committed captures (``exports/captures/``): they prove the
captures ARE genuine Django output, and the Rust replay asserts the
same bytes through the Rust worker. Regenerate with
``EXPORTS_CAPTURE=1`` (then review the diff — a capture change is a
Django behavior change).
"""

from __future__ import annotations

import io
import os
import zipfile

import pytest

from _harness import celery_wire
from _harness.db import wait_for_condition as wait_for
from _harness.export_worker import EXPORT_TASKS, spawn_export_worker
from _harness.seed import Seed

from . import cases

CAPTURES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "captures")

# Fixed capture tags: deterministic seeds (names carry the tag) make the
# exported bytes stable across runs; each capture case gets its own tag
# so the suites never collide on the workspace slug. Random tags isolate
# the behavior tests instead.
def _capture_tag(case: str) -> str:
    return f"cap-{case}"


@pytest.fixture(scope="session")
def export_worker():
    worker = spawn_export_worker()
    yield worker
    worker.stop()


@pytest.fixture()
def seed(db_conn):
    helper = Seed(db_conn)
    yield helper
    cases.cleanup_export_world(helper)
    helper.cleanup()


def _capture(name: str) -> str:
    with open(os.path.join(CAPTURES, name), "rb") as handle:
        return handle.read().decode("utf-8")


def _maybe_capture(name: str, content: bytes) -> bytes:
    """In capture mode write ``content``; otherwise assert equality."""
    if os.environ.get("EXPORTS_CAPTURE") == "1":
        os.makedirs(CAPTURES, exist_ok=True)
        with open(os.path.join(CAPTURES, name), "wb") as handle:
            handle.write(content)
        return content
    expected = _capture(name).encode("utf-8")
    assert content == expected, f"capture mismatch: {name}"
    return content


def _wait_exporter(token: str, db_conn, what: str) -> dict:
    def settled():
        row = cases.read_exporter(db_conn, token)
        # Terminal only: "processing" is transient on both backends.
        return row if row["status"] in ("completed", "failed") else None

    return wait_for(settled, what=what)


def _publish_issue_export(*, provider, world, token, multiple=False):
    return celery_wire.publish(
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


def test_oracle_worker_registers_export_tasks(export_worker):
    assert export_worker.proc.poll() is None, "oracle worker died"
    assert len(EXPORT_TASKS) == 2


@pytest.mark.parametrize("provider", ["csv", "json"])
def test_oracle_issue_export_capture(export_worker, seed, db_conn, provider):
    """Byte-identical issue exports (csv/json) through Django."""
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
    _publish_issue_export(provider=provider, world=world, token=token)
    row = _wait_exporter(token, db_conn, f"issue export {provider}")
    assert row["status"] == "completed", row
    assert row["reason"] == ""
    key = row["key"]
    assert key == cases.expected_s3_key(world, token), key

    client = cases.s3_client()
    zipped = cases.get_object_bytes(client, key)
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        names = archive.namelist()
        assert len(names) == 1, names
        inner = archive.read(names[0])
    _maybe_capture(f"issue_{provider}.golden", inner)

    # The counts BUG-EXPORT-1 drops are absent from the payload.
    text = inner.decode("utf-8", "replace")
    assert "sub_issues_count" not in text
    assert "link_count" not in text
    assert "attachment_count" not in text
    # The task queryset filters on neither state group nor draft:
    # all four issues ship, newest first.
    assert f"Draft {tag}" in text
    assert f"Triage {tag}" in text
    assert text.index(f"Draft {tag}") < text.index(f"Triage {tag}")
    assert text.index(f"Triage {tag}") < text.index(f"Child {tag}")
    assert text.index(f"Child {tag}") < text.index(f"Export me {tag}")

    presigned = cases.parse_presigned(row["url"])
    assert presigned["expires"] == "604800"
    assert presigned["algorithm"] == "AWS4-HMAC-SHA256"
    assert presigned["signed_headers"] == "host"
    assert presigned["has_signature"]
    assert presigned["path"].endswith(f"/{key}"), presigned


def test_oracle_issue_export_xlsx_cells(export_worker, seed, db_conn):
    """xlsx exports match cell-by-cell (zip bytes are library metadata)."""
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
    _publish_issue_export(provider="xlsx", world=world, token=token)
    row = _wait_exporter(token, db_conn, "issue export xlsx")
    assert row["status"] == "completed", row
    client = cases.s3_client()
    zipped = cases.get_object_bytes(client, row["key"])
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        inner = archive.read(archive.namelist()[0])
    import json as jsonlib

    cells = cases.xlsx_cells(inner)
    assert len(cells) >= 2, "header + rows"
    payload = (jsonlib.dumps(cells, indent=2, sort_keys=True) + "\n").encode("utf-8")
    _maybe_capture("issue_xlsx.cells.json", payload)


def test_oracle_issue_export_bad_provider(export_worker, seed, db_conn):
    """Unknown provider: failed + the ValueError reason (no upload)."""
    world = cases.seed_export_world(seed, cases.unique_tag("bad"))
    token = cases.unique_tag("tok")
    cases.seed_exporter(
        seed,
        workspace_id=world["workspace"],
        provider="xml",
        token=token,
        initiated_by_id=world["owner"],
    )
    _publish_issue_export(provider="xml", world=world, token=token)
    row = _wait_exporter(token, db_conn, "bad provider export")
    assert row["status"] == "failed", row
    assert row["reason"] == cases.BAD_PROVIDER_REASON, row
    assert row["url"] is None
    assert row["key"] == ""


def test_oracle_issue_export_multiple(export_worker, seed, db_conn):
    """Multiple mode zips one file per project id."""
    world = cases.seed_export_world(seed, cases.unique_tag("mul"))
    token = cases.unique_tag("tok")
    cases.seed_exporter(
        seed,
        workspace_id=world["workspace"],
        provider="csv",
        token=token,
        initiated_by_id=world["owner"],
    )
    _publish_issue_export(provider="csv", world=world, token=token, multiple=True)
    row = _wait_exporter(token, db_conn, "multiple export")
    assert row["status"] == "completed", row
    client = cases.s3_client()
    zipped = cases.get_object_bytes(client, row["key"])
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        names = archive.namelist()
    assert len(names) == 1, names
    assert names[0].endswith(".csv"), names


def _publish_analytic(*, email, data, slug):
    return celery_wire.publish(
        cases.ANALYTIC_EXPORT_TASK,
        kwargs={"email": email, "data": data, "slug": slug},
    )


def test_oracle_analytic_export_flat_capture(export_worker, seed, db_conn, smtp_sink):
    """Flat plot CSV mail through Django (byte-identical attachment)."""
    tag = _capture_tag("flat")
    world = cases.seed_export_world(seed, tag)
    email = f"flat-{tag}@example.com"
    smtp_sink.clear()
    _publish_analytic(
        email=email,
        data={"x_axis": "priority", "y_axis": "issue_count"},
        slug=world["slug"],
    )
    (delivered,) = smtp_sink.wait_for_count(1, what="analytic export mail")
    assert delivered["subject"] == "Your Export is ready"
    assert delivered["rcpt_tos"] == [email]
    parsed = cases.parse_mail(delivered["raw"])
    assert len(parsed["attachments"]) == 1
    filename, content = parsed["attachments"][0]
    assert filename == f"{world['slug']}-analytics.csv"
    _maybe_capture("analytic_flat.csv.golden", content)


def test_oracle_analytic_export_segmented_capture(export_worker, seed, db_conn, smtp_sink):
    """Segmented plot CSV mail through Django (byte-identical)."""
    tag = _capture_tag("seg")
    world = cases.seed_export_world(seed, tag)
    email = f"seg-{tag}@example.com"
    smtp_sink.clear()
    _publish_analytic(
        email=email,
        data={"x_axis": "priority", "y_axis": "issue_count", "segment": "state_id"},
        slug=world["slug"],
    )
    (delivered,) = smtp_sink.wait_for_count(1, what="segmented analytic mail")
    parsed = cases.parse_mail(delivered["raw"])
    (filename, content), = parsed["attachments"]
    assert filename == f"{world['slug']}-analytics.csv"
    _maybe_capture("analytic_segmented.csv.golden", content)


def test_oracle_analytic_export_longline_quoted_printable(
    export_worker, seed, db_conn, smtp_sink
):
    """Wide segmented CSV goes quoted-printable on the wire (PIDASHCONV-1691)."""
    world = cases.seed_export_world(seed, cases.unique_tag("qp"))
    cases.seed_label_fanout(seed, world)
    email = f"qp-{world['slug']}@example.com"
    smtp_sink.clear()
    _publish_analytic(
        email=email,
        data={"x_axis": "priority", "y_axis": "issue_count", "segment": "labels__id"},
        slug=world["slug"],
    )
    (delivered,) = smtp_sink.wait_for_count(1, what="long-line analytic mail")
    assert delivered["subject"] == "Your Export is ready"
    assert delivered["rcpt_tos"] == [email]
    cases.assert_quoted_printable_csv_mail(delivered["raw"], world["slug"])


def test_oracle_analytic_export_estimate(export_worker, seed, db_conn, smtp_sink):
    """Estimate plots mail the summed points (NULL estimates stay empty)."""
    world = cases.seed_export_world(seed, cases.unique_tag("est"))
    email = f"est-{world['slug']}@example.com"
    smtp_sink.clear()
    _publish_analytic(
        email=email,
        data={"x_axis": "priority", "y_axis": "estimate"},
        slug=world["slug"],
    )
    (delivered,) = smtp_sink.wait_for_count(1, what="estimate analytic mail")
    parsed = cases.parse_mail(delivered["raw"])
    (filename, content), = parsed["attachments"]
    text = content.decode("utf-8")
    assert '"Priority","Estimate"' in text.splitlines()[0]
    assert '"5.0"' in text, text
