-- F37-03 companion SQL — users.golden.json commands.
-- Representative Postgres statements Django emits for each lookup/write.
-- Sources: activate_user.py, reset_password.py, create_instance_admin.py,
-- create_project_member.py (see users.golden.json for branch mapping).

-- activate_user / reset_password user lookup (:28 / :35)
SELECT * FROM users WHERE email = %s LIMIT 1;

-- activate_user write (:35-36: is_active=True + full save())
UPDATE users SET is_active = TRUE, /* + all other columns via save() */
    updated_at = NOW()
WHERE id = %s;

-- reset_password write (:62-64: set_password + is_password_autoset=False + save())
UPDATE users SET password = %s, is_password_autoset = FALSE, /* + save() columns */
    updated_at = NOW()
WHERE id = %s;

-- create_instance_admin: instance + get_or_create (:32-35)
SELECT * FROM instances ORDER BY created_at DESC LIMIT 1;  -- .last()
SELECT * FROM instance_admins WHERE user_id = %s AND instance_id = %s AND role = 20 LIMIT 1;
INSERT INTO instance_admins (user_id, instance_id, role, created_at, updated_at, /* base cols */)
VALUES (%s, %s, 20, NOW(), NOW(), /* ... */);

-- create_project_member lookups (:41-52)
SELECT * FROM users WHERE email = %s LIMIT 1;
SELECT * FROM projects WHERE id = %s LIMIT 1;
SELECT 1 FROM workspace_members
WHERE workspace_id = %s AND member_id = %s AND is_active = TRUE LIMIT 1;

-- create_project_member upsert (:55-62; role is NULL when --role absent — see BUGS)
SELECT 1 FROM project_members WHERE project_id = %s AND member_id = %s LIMIT 1;
UPDATE project_members SET is_active = TRUE, role = %s, updated_at = NOW()
WHERE project_id = %s AND member_id = %s;
-- or:
INSERT INTO project_members (project_id, member_id, role, is_active, created_at, updated_at, /* base cols */)
VALUES (%s, %s, %s, TRUE, NOW(), NOW(), /* ... */);

-- create_project_member property (:65)
SELECT * FROM project_user_properties WHERE user_id = %s AND project_id = %s LIMIT 1;
INSERT INTO project_user_properties (user_id, project_id, created_at, updated_at, /* base cols */)
VALUES (%s, %s, NOW(), NOW(), /* ... */);
