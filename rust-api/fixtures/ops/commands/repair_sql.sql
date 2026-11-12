-- F37-05 repair SQL — copy_issue_comment_to_description, fix_duplicate_sequences,
-- update_deleted_workspace_slug. Representative Postgres statements Django emits.
-- Row-shape notes live in repair.rows.json.

-- copy_issue_comment_to_description (:18-51, batch_size=500, one tx per batch)
SELECT * FROM issue_comments
WHERE description_id IS NULL ORDER BY created_at ASC LIMIT 500;
INSERT INTO descriptions
    (created_at, updated_at, description_json, description_html,
     description_stripped, project_id, created_by_id, updated_by_id,
     workspace_id, id, /* base cols */)
VALUES (/* one row per comment, zip-aligned */);
UPDATE issue_comments SET description_id = %s WHERE id = %s;  -- per row (bulk_update)

-- fix_duplicate_sequences (:50-91)
SELECT * FROM projects WHERE UPPER(identifier) = UPPER(%s) AND workspace_id IN
    (SELECT id FROM workspaces WHERE slug = %s) LIMIT 1;  -- identifier__iexact
SELECT * FROM issues WHERE project_id = %s AND sequence_id = %s;
SELECT COUNT(*) FROM issues WHERE project_id = %s AND sequence_id = %s;
BEGIN;
SELECT pg_advisory_xact_lock(%s);  -- lock_key = convert_uuid_to_integer(project.id)
SELECT MAX(sequence) AS largest FROM issue_sequences WHERE project_id = %s;
SELECT * FROM issue_sequences WHERE project_id = %s;  -- id->row map
UPDATE issues SET sequence_id = %s WHERE id = %s;  -- per duplicate after the first
UPDATE issue_sequences SET sequence = %s WHERE id = %s;
COMMIT;

-- update_deleted_workspace_slug (:31, :62-64; all_objects includes soft-deleted)
SELECT * FROM workspaces WHERE slug = %s LIMIT 1;
BEGIN;
UPDATE workspaces SET slug = %s WHERE id = %s;  -- update_fields=["slug"]
COMMIT;
