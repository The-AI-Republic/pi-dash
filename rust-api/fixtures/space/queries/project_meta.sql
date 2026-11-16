-- queries/project_meta.sql
-- Space project/meta reads. Base: apps/api/pi_dash/space/views/.
-- SQL shapes as emitted by the Django compiler (table names from model Meta;
-- join aliases follow the ORM lookup paths cited per statement).
-- Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.

-- M1 settings get: DeployBoard.objects.get(anchor, entity_name="project")
-- views/project.py:23. Raises DeployBoard.DoesNotExist -> handle_exception
-- envelope (views/base.py:86-90) — there is NO inline 404 here; the 404
-- surfaces only via the exception path. Serialized with DeployBoardSerializer
-- (app/serializers/project.py:259-266: project_details + workspace_detail).
SELECT "deploy_boards"."id", "deploy_boards"."created_at", "deploy_boards"."updated_at",
  "deploy_boards"."created_by_id", "deploy_boards"."updated_by_id", "deploy_boards"."deleted_at",
  "deploy_boards"."workspace_id", "deploy_boards"."project_id", "deploy_boards"."entity_identifier",
  "deploy_boards"."entity_name", "deploy_boards"."anchor", "deploy_boards"."is_comments_enabled",
  "deploy_boards"."is_reactions_enabled", "deploy_boards"."intake_id", "deploy_boards"."is_votes_enabled",
  "deploy_boards"."view_props", "deploy_boards"."is_activity_enabled", "deploy_boards"."is_disabled"
  FROM "deploy_boards"
  WHERE ("deploy_boards"."deleted_at" IS NULL
    AND "deploy_boards"."anchor" = %(anchor)s
    AND "deploy_boards"."entity_name" = 'project');

-- M2 workspace boards: WorkspaceProjectDeployBoardEndpoint.get views/project.py:28-51.
-- BUG-PORT (views/project.py:32): `deploy_board = ...values_list` WITHOUT the
-- call — `deploy_board` is the bound method, so the very next attribute access
-- (`deploy_board.workspace`, :34) raises AttributeError and the endpoint ALWAYS
-- 500s via the dispatch `return exc` bug (views/base.py:199-200). Intended SQL:
SELECT "projects"."id", "projects"."identifier", "projects"."name",
  "projects"."description", "projects"."emoji", "projects"."icon_prop",
  "projects"."cover_image",
  EXISTS(SELECT 1 FROM "deploy_boards" U1 WHERE (U1."deleted_at" IS NULL
    AND U1."anchor" = %(anchor)s AND U1."project_id" = ("projects"."id")
    AND U1."entity_name" = 'project')) AS "is_public"
  FROM "projects"
  WHERE ("projects"."deleted_at" IS NULL
    AND "projects"."workspace_id" = %(workspace_id)s AND EXISTS (...));

-- M3 anchor get: DeployBoard.objects.get(workspace__slug, project_id,
-- entity_name="project") views/project.py:58-60; DeployBoardSerializer; 200.

-- M4 members: ProjectMember.objects.filter(project, workspace, is_active=True)
-- .values("id","member","member__display_name","member__avatar") verbatim
-- views/project.py:76-85. Bad anchor -> {"error": "Invalid anchor"} 404 (:69-74).
-- NOTE: member FK is nullable (:333-339) so LEFT OUTER JOIN; member=None rows
-- render null display_name/avatar.
SELECT "project_members"."id", "project_members"."member_id" AS "member",
  "users"."display_name" AS "member__display_name", "users"."avatar" AS "member__avatar"
  FROM "project_members" LEFT OUTER JOIN "users"
    ON ("project_members"."member_id" = "users"."id")
  WHERE ("project_members"."deleted_at" IS NULL
    AND "project_members"."project_id" = %(project_id)s
    AND "project_members"."workspace_id" = %(workspace_id)s
    AND "project_members"."is_active" = true)
  ORDER BY "project_members"."created_at" DESC;

-- M5 meta get: DeployBoard.objects.get(anchor, entity_name="project") (:21,
-- views/meta.py) -> project_id=deploy_board.entity_identifier (:26) ->
-- Project.objects.get(id=project_id) (:27). Either DoesNotExist ->
-- {"error": "Project is not published"} 404 (:23, :29). ProjectLiteSerializer.

-- M6 cycles: Cycle.objects.filter(workspace__slug, project_id)
-- .values("id","name") verbatim views/cycle.py:23-26. Board lookup is
-- .filter(anchor=...).first() with NO entity_name scoping (:19) — PORT (B15).
SELECT "cycles"."id", "cycles"."name" FROM "cycles"
  WHERE ("cycles"."deleted_at" IS NULL AND "cycles"."workspace_id" = %(workspace_id)s
    AND "cycles"."project_id" = %(project_id)s)
  ORDER BY "cycles"."created_at" DESC;

-- M7 modules: same shape on "modules" views/module.py:23-26; same unscoped
-- board lookup views/module.py:19 — PORT.

-- M8 states: State.objects.filter(~Q(name="Triage"), workspace__slug,
-- project_id).values("name","group","color","id","sequence") — KEY ORDER
-- name,group,color,id,sequence verbatim views/state.py:26-30. BUG-PORT: triage
-- excluded by NAME, not by group flag/is_triage (:27, B13).
SELECT "states"."name", "states"."group", "states"."color", "states"."id",
  "states"."sequence" FROM "states"
  WHERE (NOT ("states"."name" = 'Triage') AND "states"."deleted_at" IS NULL
    AND "states"."workspace_id" = %(workspace_id)s
    AND "states"."project_id" = %(project_id)s
    AND "states"."group" != 'triage')
  ORDER BY "states"."sequence" ASC;

-- M9 labels: Label.objects.filter(workspace__slug, project_id)
-- .values("id","name","color","parent") verbatim views/label.py:23-26.
SELECT "labels"."id", "labels"."name", "labels"."color", "labels"."parent_id" AS "parent"
  FROM "labels"
  WHERE ("labels"."deleted_at" IS NULL AND "labels"."workspace_id" = %(workspace_id)s
    AND "labels"."project_id" = %(project_id)s)
  ORDER BY "labels"."created_at" DESC;
