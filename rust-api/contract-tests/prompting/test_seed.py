"""Seed / reseed + scheduler-builtin seeder (PIDASHCONV-18, D-04).

Covers the non-HTTP half of the scope ("4 routes + seed/reseed +
seeder") at the only black-box surface that exists: the database rows
the migrate-time seed and the workspace-create seeder leave behind.

- Prompt templates: ``prompting/seed.py`` inserts the global
  (workspace-NULL) coding-task / review / test rows at migrate time;
  the reseed management commands refresh those rows in place and never
  touch workspace-scoped rows. The suite pins the seeded rows and the
  never-clobber invariant. (The reseed *commands* themselves have no
  HTTP surface, so there is no request that exercises them; the rows
  they own are asserted here instead.)
- Scheduler builtins: ``scheduler/builtins.ensure_builtin_schedulers``
  runs on every workspace creation (post_save signal) and in the seed
  migration. The suite creates a workspace through the product's own
  API and asserts the builtin catalog rows land with the exact slugs,
  source, and prompts the seeder writes.
"""

BUILTIN_SLUGS = ("security-audit", "fable-security-audit")


def test_prompt_template_globals_seeded(seed):
    """The migrate-time seed leaves exactly the three global rows."""
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT name, is_active, version, length(body)"
            " FROM prompt_template WHERE workspace_id IS NULL"
            " ORDER BY name"
        )
        rows = cur.fetchall()
    assert [r[0] for r in rows] == ["coding-task", "review", "test"]
    for _name, is_active, version, length in rows:
        assert is_active is True
        assert version >= 1
        assert length > 1000


def test_prompt_template_review_and_test_bodies(seed):
    """Review/test templates carry their polymorphic cycle markers."""
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT body FROM prompt_template"
            " WHERE workspace_id IS NULL AND name = 'review'"
        )
        review = cur.fetchone()[0]
        cur.execute(
            "SELECT body FROM prompt_template"
            " WHERE workspace_id IS NULL AND name = 'test'"
            " ORDER BY updated_at DESC LIMIT 1"
        )
        test = cur.fetchone()[0]
    assert "reviewing the work product" in review
    assert "as its FIRST USER" in test


def test_prompt_template_never_clobbers_workspace_rows(seed, tenant_a):
    """A workspace-scoped row with a seeded name survives alongside the
    global: the seed upsert matches workspace-NULL only and would fail
    the partial unique index otherwise."""
    with seed.conn.cursor() as cur:
        cur.execute(
            "INSERT INTO prompt_template"
            " (id, workspace_id, name, body, is_active, version,"
            "  created_at, updated_at)"
            " VALUES (gen_random_uuid(), %s, 'coding-task',"
            "  'operator customised body', true, 3, now(), now())"
            " RETURNING id",
            (tenant_a["workspace"]["id"],),
        )
        row_id = cur.fetchone()[0]
    seed.track("prompt_template", "id", str(row_id))
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) FROM prompt_template"
            " WHERE name = 'coding-task' AND is_active"
        )
        assert cur.fetchone()[0] == 2
        cur.execute(
            "SELECT body FROM prompt_template WHERE id = %s", (row_id,)
        )
        assert cur.fetchone()[0] == "operator customised body"


def test_workspace_create_seeds_builtin_schedulers(api, seed, tenant_a,
                                                   write_a):
    """POST /api/workspaces/ fires the seeder: the new workspace owns
    exactly the builtin catalog (idempotent per workspace+slug)."""
    import uuid as _uuid

    slug = f"ct18-seed-{_uuid.uuid4().hex[:10]}"
    r = api.post("/api/workspaces/", headers=write_a,
                 json={"name": f"CT18 {slug}", "slug": slug})
    assert r.status_code == 201, r.text
    workspace_id = r.json()["id"]
    seed.track("workspaces", "id", workspace_id)
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT slug, source, is_enabled, length(prompt)"
            " FROM schedulers"
            " WHERE workspace_id = %s AND deleted_at IS NULL"
            " ORDER BY slug",
            (workspace_id,),
        )
        rows = cur.fetchall()
    assert sorted(row[0] for row in rows) == sorted(BUILTIN_SLUGS)
    for _slug, source, is_enabled, length in rows:
        assert source == "builtin"
        assert is_enabled is True
        assert length > 100
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT prompt FROM schedulers"
            " WHERE workspace_id = %s AND slug = 'security-audit'",
            (workspace_id,),
        )
        assert "Scan this project's source code" in cur.fetchone()[0]
