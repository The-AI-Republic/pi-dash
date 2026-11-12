# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared seeds + effect readers for the exports suites (PIDASHCONV-1106).

``test_oracle_exports.py`` (Django worker) and ``test_rust_replay.py``
(Rust worker) seed identical worlds through this module and assert
identical effects: exported file bytes (or, for zip containers and
xlsx, byte-identical entries / cells), identical ``exporters`` rows,
and identical mail (parsed parts — MIME boundaries, dates and message
ids vary per send on both backends).
"""

from __future__ import annotations

import io
import os
import uuid
import zipfile
from datetime import datetime, timedelta, timezone
from email import message_from_string
from urllib.parse import parse_qsl, urlparse
from xml.etree import ElementTree

ISSUE_EXPORT_TASK = "pi_dash.bgtasks.export_task.issue_export_task"
ANALYTIC_EXPORT_TASK = "pi_dash.bgtasks.analytic_plot_export.analytic_export_task"

EXPORTS_TABLES = ["exporters"]

# Distinct issue timestamps (ordering is `-created_at`; ties would be
# plan-dependent on both backends, so the seeds keep them apart).
T1 = datetime(2026, 3, 1, 10, 0, 0, tzinfo=timezone.utc)
T2 = datetime(2026, 3, 2, 10, 0, 0, tzinfo=timezone.utc)
T3 = datetime(2026, 3, 3, 10, 0, 0, tzinfo=timezone.utc)
T4 = datetime(2026, 3, 4, 10, 0, 0, tzinfo=timezone.utc)


def seed_export_world(seed, tag: str) -> dict:
    """Seed a workspace with one project and four issues.

    ``i1`` carries every relation the serializer reads; ``i2`` is its
    child with a bare row; ``i3`` sits in a triage state and ``i4`` is a
    draft. The issue-export task filters on none of that, so all four
    ship (``-created_at`` order); the analytics plots exclude triage +
    draft in their own scope. Returns the ids + the ``exporters``-row
    factory inputs.
    """
    owner = seed.user(email=f"exp-owner-{tag}@example.com")["id"]
    member = seed.user(email=f"exp-member-{tag}@example.com")["id"]
    slug = f"exp-{tag}"
    workspace = seed.workspace(owner, slug=slug, name=f"Exports {tag}")["id"]
    seed.member(workspace, owner)
    seed.member(workspace, member)
    project = seed.project(workspace, identifier=f"EX{tag[:6].upper()}")
    seed.project_member(project, workspace, owner)
    seed.project_member(project, workspace, member)
    open_state = seed.state(project, workspace, name="Todo", group="unstarted")
    triage_state = seed.state(project, workspace, name="Triage", group="triage")

    i1 = seed.issue(
        workspace,
        project,
        name=f"Export me {tag}",
        sequence_id=11,
        state_id=open_state,
    )
    _stamp_issue(seed, i1, created=T1)
    i2 = seed.issue(
        workspace,
        project,
        name=f"Child {tag}",
        sequence_id=12,
        state_id=open_state,
    )
    _stamp_issue(seed, i2, created=T2)
    i3 = seed.issue(
        workspace,
        project,
        name=f"Triage {tag}",
        sequence_id=13,
        state_id=triage_state,
    )
    _stamp_issue(seed, i3, created=T3)
    i4 = seed.issue(
        workspace,
        project,
        name=f"Draft {tag}",
        sequence_id=14,
        state_id=open_state,
    )
    _stamp_issue(seed, i4, created=T4)
    with seed.conn.cursor() as cur:
        cur.execute('UPDATE issues SET parent_id = %s WHERE id = %s', (i1, i2))
        cur.execute('UPDATE issues SET is_draft = true WHERE id = %s', (i4,))
        cur.execute(
            'UPDATE issues SET priority = %s, start_date = %s WHERE id = %s',
            ("high", "2026-04-01", i1),
        )
    seed.conn.commit()

    # Relations on i1 (every leg the serializer reads).
    _link_assignee(seed, workspace, project, i1, member)
    ghost = seed.user(email=f"exp-ghost-{tag}@example.com")["id"]
    with seed.conn.cursor() as cur:
        cur.execute("UPDATE users SET is_active = false WHERE id = %s", (ghost,))
    seed.conn.commit()
    _link_assignee(seed, workspace, project, i1, ghost)
    estimate = _make_estimate(seed, workspace, project)
    point = _make_estimate_point(seed, workspace, project, estimate, "5")
    with seed.conn.cursor() as cur:
        cur.execute("UPDATE issues SET estimate_point_id = %s WHERE id = %s", (point, i1))
    seed.conn.commit()
    label = _make_label(seed, workspace, project, f"bug-{tag}")
    _link_label(seed, workspace, project, i1, label)
    cycle = _make_cycle(seed, workspace, project, owner, f"Sprint {tag}")
    _link_cycle(seed, workspace, project, i1, cycle)
    module = seed.module(workspace, project, name=f"Mod {tag}")
    seed.module_issue(workspace, project, module, i1)
    comment = seed.issue_comment(workspace, project, i1, text=f"note {tag}", actor_id=member)
    with seed.conn.cursor() as cur:
        # The serializer renders the comment timestamp: pin it, or the
        # captures drift with the wall clock.
        cur.execute(
            "UPDATE issue_comments SET created_at = %s, updated_at = %s WHERE id = %s",
            (T1, T1, comment),
        )
    seed.conn.commit()
    _add_subscriber(seed, workspace, project, i1, member)
    _add_link(seed, workspace, project, i1, f"https://example.com/{tag}", f"Spec {tag}")
    seed.issue_relation(workspace, project, i1, i2, relation_type="blocked_by")
    return {
        "owner": owner,
        "member": member,
        "ghost": ghost,
        "workspace": workspace,
        "slug": slug,
        "project": project,
        "issues": [i1, i2, i3, i4],
        "exported": [i4, i3, i2, i1],  # -created_at order; nothing excluded
    }


def _stamp_issue(seed, issue_id: str, created: datetime) -> None:
    with seed.conn.cursor() as cur:
        cur.execute(
            'UPDATE issues SET created_at = %s, updated_at = %s WHERE id = %s',
            (created, created, issue_id),
        )
    seed.conn.commit()


def _link_assignee(seed, workspace_id, project_id, issue_id, user_id) -> None:
    seed._insert(
        "issue_assignees",
        issue_id=issue_id,
        assignee_id=user_id,
        project_id=project_id,
        workspace_id=workspace_id,
        created_at=T1,
        updated_at=T1,
    )


def _make_label(seed, workspace_id, project_id, name: str) -> str:
    return seed._insert(
        "labels",
        name=name,
        description="",
        color="#ff0000",
        sort_order=65535,
        project_id=project_id,
        workspace_id=workspace_id,
        created_at=T1,
        updated_at=T1,
    )


def _link_label(seed, workspace_id, project_id, issue_id, label_id) -> None:
    seed._insert(
        "issue_labels",
        issue_id=issue_id,
        label_id=label_id,
        project_id=project_id,
        workspace_id=workspace_id,
        created_at=T1,
        updated_at=T1,
    )


def _make_cycle(seed, workspace_id, project_id, owner_id, name: str) -> str:
    return seed._insert(
        "cycles",
        name=name,
        description="",
        owned_by_id=owner_id,
        view_props="{}",
        logo_props="{}",
        timezone="UTC",
        version=1,
        sort_order=65535,
        progress_snapshot="{}",
        project_id=project_id,
        workspace_id=workspace_id,
        created_at=T1,
        updated_at=T1,
    )


def _link_cycle(seed, workspace_id, project_id, issue_id, cycle_id) -> None:
    seed._insert(
        "cycle_issues",
        issue_id=issue_id,
        cycle_id=cycle_id,
        project_id=project_id,
        workspace_id=workspace_id,
        created_at=T1,
        updated_at=T1,
    )


def _add_subscriber(seed, workspace_id, project_id, issue_id, user_id) -> None:
    seed._insert(
        "issue_subscribers",
        issue_id=issue_id,
        subscriber_id=user_id,
        project_id=project_id,
        workspace_id=workspace_id,
        created_at=T1,
        updated_at=T1,
    )


def _add_link(seed, workspace_id, project_id, issue_id, url: str, title: str) -> None:
    seed._insert(
        "issue_links",
        issue_id=issue_id,
        url=url,
        title=title,
        metadata="{}",
        project_id=project_id,
        workspace_id=workspace_id,
        created_at=T1,
        updated_at=T1,
    )


def seed_exporter(seed, *, workspace_id, provider: str, token: str, initiated_by_id: str) -> str:
    """One queued ``exporters`` row, as the export views create it."""
    return seed._insert(
        "exporters",
        token=token,
        provider=provider,
        status="queued",
        reason="",
        key="",
        type="issue_exports",
        workspace_id=workspace_id,
        initiated_by_id=initiated_by_id,
        created_at=datetime.now(timezone.utc),
        updated_at=datetime.now(timezone.utc),
    )


def _make_estimate(seed, workspace_id, project_id) -> str:
    return seed._insert(
        "estimates",
        name="Points",
        description="",
        type="points",
        last_used=False,
        project_id=project_id,
        workspace_id=workspace_id,
        created_at=T1,
        updated_at=T1,
    )


def _make_estimate_point(seed, workspace_id, project_id, estimate_id, value: str) -> str:
    return seed._insert(
        "estimate_points",
        estimate_id=estimate_id,
        key=5,
        description="",
        value=value,
        project_id=project_id,
        workspace_id=workspace_id,
        created_at=T1,
        updated_at=T1,
    )


def cleanup_export_world(seed) -> None:
    """Delete a seeded export world in FK order (children first).

    ``Seed.cleanup`` sweeps ``issues`` by workspace before its tracked
    deletes, which trips over the issue_* link rows this world seeds —
    so the exports suites drain the world first and let ``cleanup``
    finish the (now empty) tracked deletes.
    """
    workspaces = seed.tracked_ids("workspaces")
    if not workspaces:
        return
    tables = [
        "issue_assignees",
        "issue_labels",
        "cycle_issues",
        "module_issues",
        "issue_comments",
        "issue_subscribers",
        "issue_links",
        "issue_relations",
        "issues",
        "labels",
        "cycles",
        "modules",
        "estimate_points",
        "estimates",
        "states",
        "project_members",
        "projects",
        "workspace_members",
        "exporters",
        "workspaces",
    ]
    with seed.conn.cursor() as cur:
        for table in tables:
            cur.execute(
                f"DELETE FROM {table} WHERE workspace_id = ANY(%s)"
                if table != "workspaces"
                else "DELETE FROM workspaces WHERE id = ANY(%s)",
                (workspaces,),
            )
    seed.conn.commit()


def read_exporter(conn, token: str) -> dict:
    """Fetch one exporter row by token (frozen for comparison)."""
    from psycopg.rows import dict_row

    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute('SELECT * FROM exporters WHERE token = %s', (token,))
        row = cur.fetchone()
    assert row is not None, f"exporter row missing for token {token}"
    row = dict(row)
    for key, value in list(row.items()):
        if isinstance(value, datetime):
            row[key] = value.isoformat()
        elif not isinstance(value, (str, int, float, bool, type(None))):
            row[key] = str(value)
    # updated_at/created_at are wall-clock on both backends; the task
    # never rewrites them (update_fields), so they must equal the seed.
    return row


def s3_client():
    """boto3 client for the fixture S3 (same env Django reads)."""
    import boto3
    from botocore.client import Config

    endpoint = os.environ.get("AWS_S3_ENDPOINT_URL") or os.environ.get("MINIO_ENDPOINT_URL")
    return boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=os.environ.get("AWS_ACCESS_KEY_ID", "test"),
        aws_secret_access_key=os.environ.get("AWS_SECRET_ACCESS_KEY", "test"),
        region_name=os.environ.get("AWS_REGION") or "us-east-1",
        config=Config(signature_version="s3v4"),
    )


def s3_bucket() -> str:
    return os.environ.get("AWS_S3_BUCKET_NAME", "uploads")


def get_object_bytes(client, key: str) -> bytes:
    return client.get_object(Bucket=s3_bucket(), Key=key)["Body"].read()


def parse_presigned(url: str) -> dict:
    """The stable parts of a presigned URL (timestamps + signatures vary)."""
    parsed = urlparse(url)
    query = dict(parse_qsl(parsed.query))
    return {
        "scheme": parsed.scheme,
        "host": parsed.netloc,
        "path": parsed.path,
        "algorithm": query.get("X-Amz-Algorithm"),
        "expires": query.get("X-Amz-Expires"),
        "signed_headers": query.get("X-Amz-SignedHeaders"),
        "has_signature": bool(query.get("X-Amz-Signature")),
        "has_credential": bool(query.get("X-Amz-Credential")),
        "has_date": bool(query.get("X-Amz-Date")),
    }


def parse_mail(raw: str) -> dict:
    """Parsed mail parts: subject/from/to/text + attachments.

    MIME framing (boundaries, dates, message ids) varies per send on
    both backends, so only the parts are compared.
    """
    parsed = message_from_string(raw)
    text_parts: list[str] = []
    attachments: list[tuple[str, bytes]] = []
    if parsed.is_multipart():
        for part in parsed.walk():
            if part.is_multipart():
                continue
            filename = part.get_filename()
            payload = part.get_payload(decode=True) or b""
            if filename:
                attachments.append((filename, payload))
            elif part.get_content_type() == "text/plain":
                charset = part.get_content_charset() or "utf-8"
                text_parts.append(payload.decode(charset, "replace"))
    else:
        payload = parsed.get_payload(decode=True) or b""
        charset = parsed.get_content_charset() or "utf-8"
        text_parts.append(payload.decode(charset, "replace"))
    return {
        "subject": parsed.get("Subject"),
        "from": parsed.get("From"),
        "to": parsed.get("To"),
        "text": "\n".join(text_parts),
        "attachments": attachments,
    }


def _column_index(reference: str) -> int:
    """Zero-based column index of an A1 cell reference (``C5`` → 2)."""
    letters = "".join(char for char in reference if char.isalpha())
    index = 0
    for char in letters:
        index = index * 26 + (ord(char.upper()) - ord("A") + 1)
    return index - 1


def xlsx_cells(data: bytes) -> list[list[str]]:
    """Cell texts of the first worksheet, row by row (stdlib only).

    Shared-string and inline-string cells resolve; numbers/booleans
    render as stored. Cells expand by their ``r`` coordinate (missing
    cells read as ``""``): openpyxl writes dense rows while the Rust
    encoder omits blank cells, and both render the same grid. Enough
    for cell-by-cell workbook comparison (zip metadata and style
    bytes are intentionally ignored).
    """
    ns = {
        "m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    }
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        names = archive.namelist()
        sheet_name = next(
            n
            for n in names
            if n.startswith("xl/worksheets/sheet") and n.endswith(".xml")
        )
        sheet = ElementTree.fromstring(archive.read(sheet_name))
        shared: list[str] = []
        if "xl/sharedStrings.xml" in names:
            strings = ElementTree.fromstring(archive.read("xl/sharedStrings.xml"))
            for item in strings.findall("m:si", ns):
                shared.append("".join(item.itertext()))
    rows: list[list[str]] = []
    for xml_row in sheet.findall(".//m:row", ns):
        cells: list[str] = []
        for cell in xml_row.findall("m:c", ns):
            kind = cell.get("t", "n")
            value = cell.findtext("m:v", default="", namespaces=ns)
            if kind == "s":
                value = shared[int(value)] if value else ""
            elif kind == "inlineStr":
                inline = cell.find("m:is", ns)
                value = "".join(inline.itertext()) if inline is not None else ""
            elif kind == "b":
                value = "TRUE" if value == "1" else "FALSE"
            slot = _column_index(cell.get("r", ""))
            while len(cells) <= slot:
                cells.append("")
            cells[slot] = value
        rows.append(cells)
    return rows


def seed_label_fanout(seed, world: dict, count: int = 60) -> list[str]:
    """Link ``count`` 15-char labels to the world's first issue.

    A ``labels__id``-segmented export over this world has a header (and
    data row) over 998 bytes, which must go quoted-printable on the wire
    (PIDASHCONV-1691). Label/issue-link rows are covered by
    ``cleanup_export_world``.
    """
    workspace_id = world["workspace"]
    project_id = world["project"]
    issue_id = world["issues"][0]
    label_ids = []
    for index in range(count):
        label_id = _make_label(seed, workspace_id, project_id, f"qpseg-{index:09d}")
        _link_label(seed, workspace_id, project_id, issue_id, label_id)
        label_ids.append(label_id)
    return label_ids


def assert_quoted_printable_csv_mail(raw: str, slug: str) -> bytes:
    """Wire pins for a wide analytic CSV mail (PIDASHCONV-1691).

    The CSV attachment part is ``quoted-printable`` with QP-wrapped
    payload lines (<= 76), no raw wire line exceeds the 998-byte SMTP
    limit, and the decoded CSV still carries the > 998-byte line that
    triggered the encoding (guarding against a vacuous pass where the
    seeds never produced a long line). Returns the decoded CSV bytes.
    """
    from email import message_from_string

    parsed = message_from_string(raw)
    csv_parts = [
        part
        for part in parsed.walk()
        if not part.is_multipart() and part.get_content_type() == "text/csv"
    ]
    assert len(csv_parts) == 1, f"expected one CSV part, got {len(csv_parts)}"
    (part,) = csv_parts
    assert part["Content-Transfer-Encoding"] == "quoted-printable", part.items()
    assert part.get_filename() == f"{slug}-analytics.csv", part.get_filename()
    encoded = part.get_payload(decode=False)
    assert isinstance(encoded, str)
    assert max(len(line) for line in encoded.splitlines()) <= 76, encoded[:200]
    longest = max(len(line.encode("utf-8")) for line in raw.splitlines())
    assert longest <= 998, f"raw wire line of {longest} bytes exceeds the SMTP limit"
    decoded = part.get_payload(decode=True)
    assert isinstance(decoded, bytes)
    header = decoded.split(b"\r\n")[0]
    assert len(header) > 998, f"seeds produced no long line (header {len(header)} bytes)"
    return decoded


def unique_tag(prefix: str) -> str:
    return f"{prefix}{uuid.uuid4().hex[:8]}"


def utc_today() -> str:
    return datetime.now(timezone.utc).date().isoformat()


# `export_task.py:46`: f"{workspace_id}/export-{slug}-{token[:6]}-{today}.zip".
def expected_s3_key(world: dict, token: str) -> str:
    return f"{world['workspace']}/export-{world['slug']}-{token[:6]}-{utc_today()}.zip"


# `DataExporter` ValueError text (`porters/exporter.py:46`), stored
# verbatim as the exporter reason for unknown providers.
BAD_PROVIDER_REASON = "Unsupported format: xml. Available: ['csv', 'json', 'xlsx']"
