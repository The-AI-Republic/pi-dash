# TRACE — D-25 app:project + states + estimates fixtures (PIDASHCONV-562)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from (drift baseline): `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(zero diff Ported-from→HEAD on all sources below, verified 2026-10-02).
Gate: PIDASHCONV-83 (`rust-api/contract-tests/app_project/`, 44+15+13+8 = 80 tests);
key sets cross-checked against `test_project.py` (`LIST_KEYS`, `DETAIL_KEYS`,
`INVITE_KEYS`, `MEMBER_ROLE_KEYS`, `MEMBER_ADMIN_KEYS`, `BOARD_KEYS`),
`test_state.py` (`STATE_KEYS`), `test_estimate.py` (`ESTIMATE_KEYS`, `POINT_KEYS`).

Method (all files): every value produced by running the named Python code —
Django shell probes for serializers/models/queries/guards/tasks shapes, live
HTTP (httpx) against Django runserver for handler goldens and the permission
matrix, Celery protocol-v2 messages read off the redis broker for task
payloads. Per-file `_method` notes which. Environment: `pi_dash.settings.test`
on scratch Postgres `pidash_562_fixtures` (own cluster 127.0.0.1:5434),
`BASE_URL` http://127.0.0.1:8460, redis db 14, `CLOUD_AGENT_ENABLED` unset
(False). Django 4.2.30, djangorestframework 3.15.2. IDs/datetimes are live
values (volatile); key sets, key order, statuses and error bodies are the pins.

Out of scope (recorded as shapes only, owned elsewhere): D-24 workspace/user
views, D-26 issue views, D-32 intake views, D-07/D-08 task implementations,
D-11 `agent_execution` internals (shapes consumed verbatim).

## FX-APROJ-01 serializers/project core

- `FX-APROJ-01.serializers_project.json` — `ProjectSerializer`
  `app/serializers/project.py:30-117` (declared `:31-40`, `Meta` `:42-45`,
  `validate_name` `:47-64`, `validate_identifier` `:66-83`, `validate`
  `:85-108`, `create` `:110-117`); `ProjectLiteSerializer` `:120-133`;
  `ProjectListSerializer` `:136-168`; `ProjectDetailSerializer` `:171-190`;
  `DeployBoardSerializer` `:259-266`; pattern
  `db/models/project.py:226`; D-11 `core/agent_execution.py:34-43,83-130`,
  `cloud_agent/api.py:5-8`; `utils/content_validator.py:211-241`.
  `ProjectDetailSerializer` goldens live ONLY here (split-review
  clarification: none in FX-APROJ-04).

## FX-APROJ-02 serializers/member+invite

- `FX-APROJ-02.serializers_member_invite.json` — `ProjectMemberSerializer`
  `app/serializers/project.py:193-200`, `ProjectMemberPreferenceSerializer`
  `:203-212`, `ProjectMemberAdminSerializer` `:215-222`,
  `ProjectMemberRoleSerializer` `:225-231`, `ProjectMemberInviteSerializer`
  `:234-240`; nested `UserLiteSerializer`/`UserAdminLiteSerializer`
  `app/serializers/user.py:141-170`, `WorkspaceLiteSerializer`
  `app/serializers/workspace.py:79-83`; `fields=`-ignored proof via
  `DynamicBaseSerializer` `app/serializers/base.py:12-31`.

## FX-APROJ-03 serializers/state+estimate

- `FX-APROJ-03.serializers_state_estimate.json` — `StateSerializer`
  `app/serializers/state.py:12-34` (triage veto `:31-34`), `StateLiteSerializer`
  `:37-41`; `EstimateSerializer` `app/serializers/estimate.py:13-17`,
  `EstimatePointSerializer` `:20-32`, `EstimateReadSerializer` `:35-41`.

## FX-APROJ-04 serializers/shared

- `FX-APROJ-04.serializers_shared.json` — `WorkspaceEstimateSerializer`
  `app/serializers/estimate.py:44-50` (D-24 consumer
  `app/views/workspace/estimate.py:32`), `ProjectMemberLiteSerializer`
  `app/serializers/project.py:249-256` (D-26 consumer
  `app/views/issue/subscriber.py:56`), `ProjectIdentifierSerializer` `:243-246`,
  `ProjectPublicMemberSerializer` `:269-273`.

## FX-APROJ-05 models

- `FX-APROJ-05.models.json` — `ROLE` `db/models/project.py:28-31` (vs
  `app/permissions/base.py:13-16`, no drift), `ProjectNetwork` `:34-40`,
  `Project` `:72-299` (`resolve`/`resolve_id` `:191-224`,
  `FORBIDDEN_IDENTIFIER_CHARS_PATTERN` `:226`, `Meta` `:228-253`, `save`
  `:255-299`), `ProjectBaseModel` `:302-311`, `ProjectMemberInvite` `:314-329`,
  `ProjectMember` `:332-382`, `ProjectIdentifier` `:386-403`,
  `ProjectDeployBoard` `:422-439`, `ProjectPublicMember` `:442-461`,
  `ProjectUserProperty` `:464-494`; `StateGroup`/`DEFAULT_STATES`/managers/`State`
  `db/models/state.py:14-22,:26-76,:79-90,:93-140`; `EstimateType`/`Estimate`/
  `EstimatePoint` `db/models/estimate.py:13-57`; `DeployBoard`
  `db/models/deploy_board.py:19-57`; audit columns `db/mixins.py:16-70`, pk
  `db/models/base.py:17-18`; referenced-not-owned column subsets for
  Workspace/WorkspaceMember/User/UserFavorite/Intake/Issue/IssueSequence.

## FX-APROJ-06 queries

- `FX-APROJ-06.queries.json` — `ProjectViewSet.get_queryset`
  `app/views/project/base.py:52-98`, `list_detail` `:101-142`, `list` `:144-223`
  (`.values()` `:174-197`: 22 keys, not 26); member queryset
  `app/views/project/member.py:33-44`; invite querysets
  `app/views/project/invite.py:43-51,:120-126`; favorites queryset
  `app/views/project/base.py:506-514`; state queryset
  `app/views/state/base.py:31-46`; estimate list
  `app/views/estimate/base.py:54-61`; roles `.values`
  `app/views/project/member.py:346-356`; `resolve` SQL
  `db/models/project.py:197-212`.

## FX-APROJ-07 guards

- `FX-APROJ-07.guards.json` — `ROLE` + `allow_permission`
  `app/permissions/base.py:13-87` (call sites project/base.py
  `:100,:144,:225,:257,:433,:441,:450,:461`, invite.py `:53,:128`, member.py
  `:46,:156,:171,:205,:267,:300,:367,:379`, state/base.py
  `:49,:66,:84,:112,:122,:148`, estimate/base.py `:35,:154,:170,:185`),
  `ProjectMemberPermission`/`ProjectEntityPermission`/`can_mutate_states`
  `app/permissions/project.py:56-116,:146-184`, `WorkspaceUserPermission`
  `app/permissions/workspace.py:103-110`, `generate_cache_key`/
  `invalidate_cache_directly` `utils/cache.py:15-88`, `base_host`
  `utils/host.py:17-70`, 401 bodies (DRF `IsAuthenticated` +
  `authentication/adapter/exception.py:17-32`).

## FX-APROJ-08 tasks

- `FX-APROJ-08.tasks.json` — call sites project/base.py `:246-252`
  (`recent_visited_task`), `:300-308`/`:369-377` (`model_activity`),
  `:405-417` (`webhook_activity`), member.py `:143-150`
  (`project_add_user_email`), estimate/base.py `:198-210`/`:217-228`
  (`issue_activity`); invite JWT invite.py `:80-84`; 500 paths invite.py `:62`
  (raises first) and `:104` (unreachable), favorites list missing
  `serializer_class` project/base.py `:503-514`; `handle_exception`
  `app/views/base.py:110-149` (viewset) / `:211-248` (apiview); dispatch
  outer-except `:151-163`/`:250-262` (no wire effect, see file).

## FX-APROJ-09 handlers/project+members+invites

- `FX-APROJ-09.handlers_project.json` — routes `app/urls/project.py:24-131`
  (20 paths, resolver-verified); `ProjectViewSet`
  `app/views/project/base.py:46-430`, `ProjectArchiveUnarchiveEndpoint`
  `:432-446`, `ProjectIdentifierEndpoint` `:449-476`,
  `ProjectUserViewsEndpoint` `:479-500`, `ProjectFavoritesViewSet` `:503-537`,
  `DeployBoardViewSet` `:540-581` (+ `WorkspaceBaseModel.save` backfill
  `db/models/workspace.py`), `ProjectInvitationsViewset`/
  `UserProjectInvitationsViewset`/`ProjectJoinEndpoint`
  `app/views/project/invite.py:37-254`, `ProjectMemberViewSet`/
  `ProjectMemberUserEndpoint`/`UserProjectRolesEndpoint`/
  `ProjectMemberPreferenceEndpoint` `app/views/project/member.py:27-385`.

## FX-APROJ-10 handlers/states+estimates

- `FX-APROJ-10.handlers_states_estimates.json` — routes
  `app/urls/state.py:11-32` (4 paths), `app/urls/estimate.py:15-41` (5 paths);
  `StateViewSet`/`IntakeStateEndpoint` `app/views/state/base.py:27-157`,
  `generate_random_name` + estimate endpoints
  `app/views/estimate/base.py:29-247`.

## Ported bugs (also listed in the PR)

1. `DynamicBaseSerializer` ignores `fields=` (`app/serializers/base.py:17-20`:
   `fields` overwritten by `expand`): member list + guest retrieve render the
   full 6-key shape (FX-APROJ-02/09).
2. Invite create `invite.py:62` calls `.role` on a QuerySet → AttributeError →
   500 with 0 rows persisted; the `.delay`-on-list bug at `:104` is therefore
   unreachable (FX-APROJ-08).
3. Favorites list has no `serializer_class` → DRF-default list raises →
   generic 500 (FX-APROJ-06/08).
4. Bulk estimate create persists the Estimate row before points validation
   (`app/views/estimate/base.py:69` vs `:78-80`): a 400 still creates a
   point-less estimate (FX-APROJ-10).
5. Point create requires truthy key (`:157`): `key=0` 400s though the model
   default is 0 (FX-APROJ-10).
6. Member self-patch guard (`app/views/project/member.py:216`) fires for any
   field, not just role (FX-APROJ-09).
7. `BaseViewSet.dispatch`/`BaseAPIView.dispatch` outer-except returns `exc`
   (`app/views/base.py:161-163,:260-262`): no wire effect — the inner
   `handle_exception` never re-raises, so every error renders JSON
   (FX-APROJ-08).
