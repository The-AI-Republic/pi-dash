"""Domain seeding for the D-31 oracle: ``file_assets`` rows.

Column lists mirror the Django schema (``file_assets``). Raw-SQL seeding
bypasses model signals by design: each test sees exactly the rows it
inserted. Keys mirror the view convention (``<workspace_id>/<hex>-<name>``
for workspace assets, ``user-<hex>-<name>`` for user assets).
"""

from __future__ import annotations

import json
import uuid

from _harness import seed


def create_asset(
    conn,
    *,
    workspace_id: str | None,
    created_by_id: str,
    name: str,
    entity_type: str | None = None,
    entity_identifier: str | None = None,
    user_id: str | None = None,
    project_id: str | None = None,
    file_type: str = "image/png",
    size: float = 100,
    uploaded: bool = False,
    deleted: bool = False,
    with_deleted_at: bool = False,
) -> dict:
    """Insert a ``file_assets`` row; return ``{id, key}``."""
    aid = seed.new_id()
    now = seed.now_iso()
    if workspace_id is not None:
        key = f"{workspace_id}/{uuid.uuid4().hex}-{name}"
    else:
        key = f"user-{uuid.uuid4().hex}-{name}"
    attributes = json.dumps({"name": name, "type": file_type, "size": size})
    deleted_at = now if (deleted and with_deleted_at) else None
    conn.execute(
        """INSERT INTO file_assets (id, attributes, asset, created_by_id,
            workspace_id, user_id, project_id, entity_type, entity_identifier,
            is_deleted, deleted_at, is_archived, is_uploaded, size,
            storage_metadata, created_at, updated_at)
        VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,false,%s,%s,'{}',%s,%s)""",
        (
            aid, attributes, key, created_by_id, workspace_id, user_id,
            project_id, entity_type, entity_identifier, deleted, deleted_at,
            uploaded, size, now, now,
        ),
    )
    return {"id": aid, "key": key}


def mark_uploaded(conn, asset_id: str) -> None:
    conn.execute(
        "UPDATE file_assets SET is_uploaded = true WHERE id = %s", (asset_id,)
    )


def fetch(conn, asset_id: str) -> dict:
    import uuid as _uuid

    row = conn.execute(
        """SELECT id, asset, workspace_id, user_id, project_id, entity_type,
            entity_identifier, attributes, size, is_uploaded, is_deleted,
            deleted_at FROM file_assets WHERE id = %s""",
        (asset_id,),
    ).fetchone()
    keys = (
        "id", "asset", "workspace_id", "user_id", "project_id",
        "entity_type", "entity_identifier", "attributes", "size",
        "is_uploaded", "is_deleted", "deleted_at",
    )
    out = dict(zip(keys, row))
    for key in ("id", "workspace_id", "user_id", "project_id"):
        if isinstance(out[key], _uuid.UUID):
            out[key] = str(out[key])
    return out
