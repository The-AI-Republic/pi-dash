# TRACE — D-19 api-v1 projects/members/states/estimates fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(zero diff Ported-from→HEAD on all sources below, verified 2026-09-29).

## Serializers (FX-PROJ-SER, FX-COLLAB-SER, FX-WORKFLOW-SER)

- `serializers/project.golden.json` — `api/serializers/project.py:24-48`
  (`_validate_default_agent_executor`), `:49-196` (ProjectCreateSerializer:
  Meta `:98-137`, validate `:139-167`, create `:168-194`),
  `:197-250` (ProjectUpdateSerializer: Meta `:205-213`, update `:214-248`),
  `:251-353` (ProjectSerializer: annotations `:259-266`, validate `:283-330`,
  create `:331-351`), `:354-377` (ProjectLiteSerializer); identifier-taken
  error branches `api/views/project.py:272-284` (create) and `:455-467`
  (patch); pattern `FORBIDDEN_IDENTIFIER_CHARS_PATTERN`
  `db/models/project.py:226`; save backstops `db/models/project.py:255-299`;
  AgentExecutorKind `core/agent_execution.py:7-16`.
- `serializers/collab.golden.json` — `api/serializers/member.py:15-43`
  (validate_member `:25-34`, validate_role `:35-38`); `api/serializers/invite.py:16-60`
  (validate_email `:41-47`, validate_role `:48-52`, validate `:53-60`);
  `api/serializers/user.py:13-38` (UserLiteSerializer, avatar_url `:21-24`);
  avatar_url property `db/models/user.py:142-151`; ROLE values
  `app/permissions/base.py:13-16`.
- `serializers/workflow.golden.json` — `api/serializers/state.py:11-55`
  (StateSerializer.validate `:19-27`, StateLiteSerializer `:44-55`);
  `api/serializers/estimate.py:13-37` (EstimateSerializer.create `:19-24`,
  EstimatePointSerializer.validate `:26-33`); State.save/sequence/slug
  `db/models/state.py:131-139`; workspace backfill
  `db/models/project.py:309-311`.

## Models (FX-MODELS)

- `models/project.columns.json` — `db/models/project.py:72-301` (+ audit cols
  `db/mixins.py:16-69`, UUID pk `db/models/base.py:17-21`).
- `models/project_member.columns.json` — `db/models/project.py:332-385`
  (save side-effect `:348-364`).
- `models/project_member_invite.columns.json` — `db/models/project.py:314-331`
  (no D-19 view/serializer touches this table).
- `models/project_identifier.columns.json` — `db/models/project.py:386-403`
  (AutoField integer pk — AuditModel declares no id; written only by
  ProjectSerializer.create `api/serializers/project.py:346-350`).
- `models/state.columns.json` — `db/models/state.py:93-140` (managers `:79-91`,
  DEFAULT_STATES `:26-76`).
- `models/estimate.columns.json` — `db/models/estimate.py:18-40`.
- `models/estimate_point.columns.json` — `db/models/estimate.py:43-57`.
- `models/workspace_member_invite.columns.json` — `db/models/workspace.py:234-280`
  (approx; class `:234-258`).
- `models/user_favorite.columns.json` — `db/models/favorite.py:14-60`
  (approx; project touch points `api/views/project.py:493,537`).
- `models/identifier_routing.golden.json` — `Project.resolve/resolve_id`
  `db/models/project.py:191-224`; `_rewrite_project_kwarg`
  `api/views/base.py:52-104`; `project_id` property
  `api/views/base.py:203-211`.

## Queries (FX-Q-PROJMEM, FX-Q-STATEEST)

- `queries/project_member.sql` + `.rows.json` — project list queryset
  `api/views/project.py:82-140`, GET sort_order/prefetch `:169-187`, detail
  queryset `:297-371`; member querysets `api/views/member.py:82,135-141,189-192`;
  invite queryset `api/views/invite.py:36-40`; user `api/views/user.py:40`.
- `queries/state_estimate.sql` + `.rows.json` — state querysets
  `api/views/state.py:47-60,170-183`, direct gets `:231,278`; estimate querysets
  `api/views/estimate.py:35-36,144-149,241-246`, parent checks
  `:48-54,169-173,197-201`.

## Guards (FX-PERMS)

- `guards/permissions.golden.json` — `app/permissions/project.py:13-55`
  (ProjectBasePermission), `:56-84` (ProjectMemberPermission), `:85-118`
  (ProjectEntityPermission), `:119-186` (ProjectAdminPermission +
  ProjectLitePermission `:133-143`, unused by D-19), `:146-206`
  (can_mutate_states + ProjectStateEntityPermission);
  `app/permissions/workspace.py:51-60` (WorkspaceOwnerPermission),
  `:61-110` (WorkSpaceAdminPermission); name resolution
  `api/views/member.py:18`, `api/views/invite.py:20`,
  `api/views/project.py:48`, `api/views/state.py:15`,
  `api/views/estimate.py:11`; utils duplicates
  `utils/permissions/{project,workspace,base}.py` (project copy lacks the
  state section; workspace copy identical); `get_permissions`
  `api/views/member.py:98-101`.

## Tasks (FX-TASKS)

- `tasks/project_activity.before_after.json` — call sites
  `api/views/project.py:258-270` (create), `:442-460` (update),
  `:480-510` (delete + webhook Activity `:495-507`); task bodies
  `bgtasks/webhook_task.py:253-260,377-390` (webhook_activity),
  `:463-508` (model_activity); origin `utils/host.py:17-25`.

## Handlers (FX-H-PROJ, FX-H-MEM, FX-H-STATEEST)

- `handlers/project.golden.json` — `api/views/project.py:163-213` (get),
  `:214-285` (post), `:373-402` (detail get), `:403-479` (patch),
  `:480-509` (delete), `:528-551` (archive post), `:552-574` (unarchive
  delete), `:580-603` (summary get), `:605-677` (counts); routes
  `api/urls/project.py` (4 entries); envelope cross-checked against
  `rust-api/contract-tests/v1_projects/test_projects.py`.
- `handlers/collab.golden.json` — `api/views/member.py:31-93`
  (workspace list), `:94-159` (list/create), `:160-222` (detail);
  `api/views/invite.py:24-154` (queryset `:36-38`, get_object `:39-54`,
  list `:55-74`, retrieve `:75-88`, create `:89-111`, partial_update
  `:112-140`, destroy `:141-154`); `api/views/user.py:18-41`; routes
  `api/urls/member.py` (5 entries), `api/urls/invite.py` (1 router prefix),
  `api/urls/user.py` (1 entry); cross-checked against
  `contract-tests/v1_projects/test_{members,invites,user}.py`.
- `handlers/workflow.golden.json` — `api/views/state.py:39-160`
  (queryset `:47-79`, post `:80-148`, get `:149-160`), `:162-300`
  (queryset `:170-199`, get `:200-224`, delete `:225-271`, patch `:272-300`);
  `api/views/estimate.py:30-136` (queryset `:35-46`, post `:47-82`, get
  `:83-105`, patch `:106-128`, delete `:129-136`), `:137-233` (queryset
  `:144-167`, get `:168-195`, post `:196-233`), `:234-291` (queryset
  `:241-263`, patch `:264-285`, delete `:286-291`); routes
  `api/urls/state.py` (2 entries), `api/urls/estimate.py` (3 entries,
  UNREGISTERED — `api/urls/__init__.py` has no estimate import);
  cross-checked against
  `contract-tests/v1_projects/test_{states,estimates}.py`.

## Cross-cutting ported bugs (translate as-is)

- BUG-1: identifier-taken on the create path surfaces as the NAME conflict
  body (ProjectCreateSerializer.create never writes the ProjectIdentifier row
  its own pre-check reads; the clash trips the projects unique index instead).
- BUG-2: estimate routes all 404 — patterns defined but never registered.
- BUG-3: `UserLiteSerializer.Meta.fields` lists `email` twice (harmless).
- BUG-4: State default-flip runs inside validate() — siblings lose default
  even if the save later fails; caller-supplied sequence is overwritten by
  max+15000 on create.
- BUG-5: state patch echoes the TARGET row id (not the conflicting row's) on
  external-id 409; state patch/delete skip the archived-project guard.
- BUG-6: WorkspaceOwnerPermission has no is_active filter — inactive admins pass.
- BUG-7: ProjectMemberPermission SAFE branch is workspace-scoped, not
  project-scoped — any project membership in the workspace reads every
  project's member list.
- BUG-8: model_activity emits only for keys present in BOTH requested_data and
  the before-image snapshot — brand-new keys never emit an "updated" event.
