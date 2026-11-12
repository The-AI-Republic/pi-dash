# TRACE — D-02 space public-API fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/space/`. Supporting modules noted where behaviour
lives outside `space/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

## Models

- `models/deployboard.columns.json` — `db/models/deploy_board.py:19-57` (class + Meta 45-57); base cols `db/models/base.py:17-18` (id), `db/mixins.py:16-20` (TimeAudit), `:26-42` (UserAudit), `:61-64` (deleted_at), managers `:56-58,:66-67`; workspace/project FKs `db/models/workspace.py:185-195` (WorkspaceBaseModel).
- `models/project_lite.columns.json` — Project `db/models/project.py:72-253` (Meta 228-253); ProjectMember `:332-378` (Meta 366-378); ProjectPublicMember `:442-461` (Meta 449-461); WorkspaceMember `db/models/workspace.py:198-227` (Meta 215-227); User identity cols `db/models/user.py:56-137` (Meta 133-137); ProjectBaseModel `db/models/project.py:302-311`.
- `models/issue_public.columns.json` — Issue `db/models/issue.py:107-265` (Meta 250-265); IssueManager `:95-104`, `issue_objects` `:229` (no `objects =` on Issue — objects = inherited `db/mixins.py:66`); IssueLink `:471-481`; IssueRelation `:396-417` (choices `:372-393`).
- `models/comment.columns.json` — IssueComment `db/models/issue.py:550-662` (TRACKED_FIELDS `:596`, Meta `:649-662`).
- `models/reaction.columns.json` — IssueReaction `db/models/issue.py:726-747`; CommentReaction `:753-774`.
- `models/vote.columns.json` — IssueVote `db/models/issue.py:780-797`.
- `models/intake.columns.json` — IntakeIssue `db/models/intake.py:42-80` (status choices `:42-47`, Meta `:76-80`).
- `models/asset.columns.json` — FileAsset `db/models/asset.py:28-74` (EntityTypeContext `:33-43`, Meta `:64-74`); managers `db/mixins.py:56-58,:66-67` (no `objects =` in asset.py — restore uses `all_objects`, `space/views/asset.py:183`).
- `models/cycle_module_state_label.columns.json` — Cycle `db/models/cycle.py:60-86`; CycleIssue `:104-124`; Module `db/models/module.py:67-113`; ModuleIssue `:152-168`; State `db/models/state.py:14-22,:93-129` (managers `:79-90,:109-111`); Label `db/models/label.py:11-44` (Meta `:26-44`).

## Serializers

- `serializers/lite_leaves.golden.json` — `space/serializer/base.py:8-9` (id rule); `user.py:10-22`; `workspace.py:10-14`; `project.py:10-22`; `state.py:10-21`; `cycle.py:10-21`; `module.py:10-21`.
- `serializers/intake.golden.json` — `space/serializer/intake.py:17-47` (bridge_id `:40`, exclude `:47`); app twin `app/serializers/intake.py:127-139` (no bridge_id — the single divergence); intake create 200 `space/views/intake.py:172-173`.
- `serializers/issue_graph.golden.json` — `space/serializer/issue.py:41-47` (IssueStateFlat), `:50-57` (Label), `:60-66` (IssueProjectLite), `:69-84` (relation pair, sources `:70`/`:79`), `:87-116` (cycle/module detail), `:119-139` (link, create guard `:136-139`), `:142-154` (attachment), `:157-161` (reaction 5-key), `:164-201` (IssueSerializer, exclude `:185`, assigned_pod `:195-201`), `:204-220` (IssueFlat 10-key), `:223-228` (CommentReactionLite), `:231-250` (comment), `:255-422` (IssueCreate: write keys `:261-271`, to_representation `:287-291`, validate `:293-315`, create `:317-374`, update `:376-422`), `:425-429` (CommentReaction), `:432-436` (vote), `:439-464` (IssuePublic), `:467-470` (LabelLite); `space/serializer/__init__.py:1-9` (partial exports); app observed shapes `app/serializers/issue.py:900-906,917-936,939-991,1021-1036,1039-1113,136-518,1415-1438`, `app/serializers/intake.py:27-90`, `app/serializers/project.py:259-266`; `space/serializer/base.py:12-62` (DynamicBaseSerializer, nested-dict recursion BUG `:41`).

## Queries

- `queries/project_meta.sql` + `.rows.json` — `space/views/project.py:19-86` (settings `:22-25`, boards BUG `:28-51`, anchor `:57-62`, members `:68-86`); `space/views/meta.py:16-32`; `space/views/cycle.py:15-28`; `space/views/module.py:15-28`; `space/views/state.py:18-32` (name-exclusion `:27`, key order `:26-30`); `space/views/label.py:15-28`.
- `queries/issue_list.sql` + `.rows.json` — `space/views/issue.py:76-211` (queryset `:87-126`, order `:129-131`, grouper wiring `:134-138`, sub-grouped `:148-178`, grouped `:181-204`, plain `:206-211`, group-equality 400 `:142-146`, count_filter `:170-177,:196-203`); `space/utils/grouper.py:28-70` (grouper, `or`-BUG `:67`), `:73-182` (on_results, required_fields `:84-99`, FIELD_MAPPER `:76-80,:101-109`, vote/reaction items `:111-180`), `:185-252` (group_values branches incl. missing-list BUG `:208` and None-queryset BUG `:233-250`).
- `queries/issue_retrieve.sql` + `.rows.json` — `space/views/issue.py:597-773` (board `:598`, Coalesce+ArrayAgg guards `:613-644`, prefetches `:645-651`, vote/reaction items `:652-745` incl. copy-paste BUG `:713,:716,:722`, 23-key values `:746-770`, first `:771`).
- `queries/comments_reactions_votes.sql` + `.rows.json` — `space/views/issue.py:214-255` (comment queryset + is_member Exists `:241-250`, `.none()` paths `:253-255`), `:257-339` (comment create/update/destroy), `:342-427` (reaction viewset, wrong-kwarg BUG `:348-351`), `:430-519` (comment-reaction viewset), `:522-591` (vote viewset, anchor-as-slug BUG `:528-530`); app serializer shapes `app/serializers/issue.py:917-991`.
- `queries/intake.sql` + `.rows.json` — `space/views/intake.py:31-280` (get_queryset dead path `:37-54`, list `:56-105` incl. bridge_id `:72` + ordering `:75` + counts `:76-96`, create `:107-173` incl. triage `:129-142`, no-workspace INSERTs `:145-152,:165-170`, 200-not-201 `:173`, partial_update `:175-234`, retrieve `:236-256`, destroy `:258-280`).
- `queries/assets.sql` + `.rows.json` — `space/views/asset.py:26-226` (get `:34-66` incl. entity_type__in `:48-51`, 302-redirect `:66`; post `:68-133` incl. size default `:78`, asset_key `:107`, unconditional comment_id `:118`; patch `:135-154` incl. metadata delay `:147-148`; delete `:156-169`; restore `:175-187` incl. all_objects `:183`; bulk `:193-226` incl. COMMENT_DESCRIPTION-only `:223-225`).

## Guards

- `guards/permissions.golden.json` — route→permission matrix `space/urls/{project.py:21-65,intake.py:15-34,issue.py:17-56,asset.py:16-35}` × `space/views/{project.py:20,29,55,66,meta.py:17,cycle.py:16,module.py:16,state.py:19,label.py:15,issue.py:74,220-226,595,asset.py:27-32,intake.py:31-35,base.py:48,134}`; every error body+status `space/views/{project.py:71-74,issue.py:82,143-146,261-264,300-303,324-327,370-373,407-410,455-458,491-494,intake.py:59-62,110-113,116,126,178-181,191-194,239-242,260-263,274-277,asset.py:39-42,56-59,73,84-87,98-104,140,161,180,198,204,217-220,cycle.py:21,module.py:21,state.py:24,label.py:21,meta.py:23,29}`; TimezoneMixin `space/views/base.py:31-42`; handle_exception `space/views/base.py:65-103,:149-186`; dispatch `return exc` BUG `:105-117,:188-200`.

## Tasks

- `tasks/enqueue.golden.json` — `issue_activity.delay` sites `space/views/issue.py:274-282,308-316,329-337,391-399,417-425,476-484` (project_id="None" BUG `:481`), `:503-517,:561-569,:581-589`, `space/views/intake.py:155-163,223-231`; `get_asset_object_metadata.delay` `space/views/asset.py:147-148`.
