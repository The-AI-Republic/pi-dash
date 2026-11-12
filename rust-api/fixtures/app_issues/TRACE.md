# TRACE — D-26 app:issues non-list fixtures (PIDASHCONV-637)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from (drift baseline):
`01a93e17216faea7bfc156b0f864cbbe420d1c52` (zero diff
Ported-from→HEAD on all sources below, verified 2026-10-02).
Gate: PIDASHCONV-84 (`rust-api/contract-tests/app_issues/`); key sets
cross-checked against `LIST_KEYS`, `RETRIEVE_KEYS` (35 keys, no
`is_intake`), `ACTIVITY_KEYS`, `COMMENT_WRITE_KEYS`, `ARCHIVE_DETAIL_KEYS`,
`ASSET_KEYS`, `LABEL_KEYS`, `REL_ROW_KEYS`, `LINK_KEYS`, `SUB_KEYS`,
`VERSION_*_KEYS`, `DESC_DETAIL_KEYS`.

Method (all files): hand-written against the cited source lines (pilot-2
precedent) — each golden was composed from the cited lines, then re-read
against them. No live Django, no record-and-freeze. Structural SQL is
derived from the ORM chain (table/join/subquery/WHERE shapes); the queries
layer runs verify byte-exact SQL against Django 4.2.30 +
djangorestframework 3.15.2. Error strings and status codes are verbatim.
Django pinned per `apps/api/requirements/base.txt`.

Out of scope here (pilot-2's, untouched): `list/` (fixtures + TRACE.md) —
anchors re-verified below, not re-recorded.

## Serializers

- `serializers/FX-ISS-01.create.json` — `IssueCreateSerializer`
  `app/serializers/issue.py:136-520` (declared `:138-167`, `Meta`
  `:169-183`, `to_representation` `:185-191`, `validate_complexity_score`
  `:193-204`, `validate` `:206-385`, `create` `:387-462`, `update`
  `:464-518`); sync lock `:216-229` + `_LOCKED_ISSUE_FIELDS` `:59` +
  `_issue_is_actively_synced` `:63-76`; pod `:244-269`; executor `:275-321`
  + `_MANAGED_UNAVAILABLE_DETAIL` `:93-102`; sanitize `:324-335`;
  assignee/label filters `:338-354`; state/parent/estimate `:357-383`;
  `AgentExecutorKind` `core/agent_execution.py:7-16`,
  `cloud_agent_is_configured` `:34-43`, `ManagedRunnerReason`
  `managed_runner/errors.py:15-33`, `NON_TERMINAL_STATUSES`
  `runner/services/matcher.py:54-66`, `validate_html_content`
  `utils/content_validator.py:211-241`.
- `serializers/FX-ISS-02.detail.json` — `IssueSerializer` base
  `app/serializers/issue.py:1039-1113` (29 keys `:1063-1093`, `validate`
  `:1099-1113`); `IssueDetailSerializer` `:1226-1414` (blocker cache
  `:1250-1266`, ticker `:1268-1318`, live-state/run `:1320-1371`, status
  `:1373-1412`); `IssueLiteSerializer` `:1219-1225`;
  `IssuePublicSerializer` `:1415-1440`; `classify_run_error`
  `runner/diagnostics.py:173-244`; ticker methods
  `db/models/issue_agent_ticker.py:185-245`.
- `serializers/FX-ISS-03.assoc.json` — `IssueAssigneeSerializer`
  `:670-677`, `IssueLabelSerializer` `:583-589`,
  `ProjectUserPropertySerializer` `:542-548`,
  `IssueVersionDetailSerializer` `:1448-1486` (dup `name` `:1459`+`:1476`),
  `IssueDescriptionVersionDetailSerializer` `:1487-1506`.
- `serializers/FX-ISS-04.links.json` —
  `GithubPullRequestLinkSerializer` `:738-768`,
  `GitCodeReviewLinkSerializer` `:769-800`, `IssueLinkLiteSerializer`
  `:852-866`, `IssueAttachmentLiteSerializer` `:884-899` (no `issue_id`
  `:891`); `asset_url` `db/models/asset.py:79-100`.
- `serializers/FX-ISS-05.engage.json` — `IssueActivitySerializer`
  `:521-541` (`get_source_data` `:528-535`),
  `IssueReactionLiteSerializer` `:909-916`, `IssueSubscriberSerializer`
  `:1441-1447`, `_issue_is_actively_synced` `:63-76`,
  `_comment_is_actively_synced` `:79-87`; comment sync-lock reference
  `IssueCommentSerializer.validate` `:974-990`.
- `serializers/FX-ISS-06.refs.json` — `CycleBaseSerializer` `:678-691`,
  `ModuleBaseSerializer` `:708-721`, `IssueStateSerializer` `:1004-1020`,
  `IssueIntakeSerializer` `:1021-1038`.

## Models

- `models/FX-ISS-07.core.json` — `IssueManager` + `Issue`
  `db/models/issue.py:95-357` (columns `:115-229`, `has_active_run`
  `:232-248`, `save` `:267-351`), `IssueAssignee` `:445-469`,
  `IssueLabel` `:669-681`, `Label` `db/models/label.py:11-57`,
  `ProjectUserProperty` `db/models/project.py:464-495`; `id`
  `db/models/base.py:17-18`; audit `db/mixins.py:16-70`; foreign reads
  State `db/models/state.py:93-111`, Project, Workspace, User,
  ProjectMember `db/models/project.py:332-382`, EstimatePoint
  `db/models/estimate.py:43-57`, IssueType `db/models/issue_type.py:14-29`.
- `models/FX-ISS-08.engage.json` — `IssueComment` `:550-667` (`save`
  `:598-647`), `CommentReaction` `:753-778`, `IssueReaction` `:726-751`,
  `IssueVote` `:780-801`, `IssueSubscriber` `:700-724`; foreign
  Description `db/models/description.py:10-20`.
- `models/FX-ISS-09.links.json` — `IssueLink` `:471-485`,
  `IssueRelationChoices` + `IssueRelation` `:372-421`,
  `GithubPullRequestLink` `db/models/integration/github.py:217-262`,
  `GitCodeReviewLink` `db/models/integration/git.py:224-272`, `FileAsset`
  `db/models/asset.py:28-100`; foreign GithubAppInstallation
  `integration/github.py:121-141`, GitIssueSync `integration/git.py:152-170`,
  GithubIssueSync `integration/github.py:76-90`, comment sync tables.
- `models/FX-ISS-10.reads.json` — `IssueActivity` `:514-548`,
  `IssueVersion` `:803-906`, `IssueDescriptionVersion` `:908-948`,
  `CycleIssue` `db/models/cycle.py:104-128`, `ModuleIssue`
  `db/models/module.py:152-172`; foreign AgentRun + `AgentRunStatus`
  `runner/models.py:207-232` (enum merged in `db/src/dispatch/` — cited),
  `RunnerLiveState` `runner/models.py:1454-1516`, `Pod`
  `runner/models.py:70-95,174-176`, `IssueAgentTicker`
  `db/models/issue_agent_ticker.py:84-245`, `IntakeIssue`
  `db/models/intake.py:42-76`, `UserRecentVisit`
  `db/models/recent_visit.py:22-36`, `IssueMention` `:423-442`,
  `IssueSequence` `:683-697`, Cycle, Module (`archived_at`
  `db/models/module.py:98`).

## Queries

- `queries/FX-ISS-11.core.json` — retrieve `app/views/issue/base.py:486-620`
  (ORM `:489-581`), partial-update fetch `:621-668`, sub-issues GET
  `app/views/issue/sub_issue.py:37-201`, relation list
  `app/views/issue/relation.py:42-207`, archive list/retrieve
  `app/views/issue/archive.py:54-255`; pilot-2 reuse `get_queryset`
  `:210-216` + `apply_annotations` `:218-257`.
- `queries/FX-ISS-12.engage.json` — history
  `app/views/issue/activity.py:30-86`, comment querysets
  `app/views/issue/comment.py:43-69,186-200`, version querysets
  `app/views/issue/version.py:36-144`, meta `base.py:1199-1211`,
  identifier `base.py:1214-1489` (lite `:1267-1310`, full `:1312-1446`);
  `paginate` `utils/global_paginator.py:1-87` (CODE is pilot-2's
  `api/src/app_issues/render.rs` — reuse, goldens only here).
- `queries/FX-ISS-13.move.json` — `move_work_item_to_project` +
  `IssueMoveError` `utils/issue_move.py:109-393` (cancel frame `:70-107`,
  guards `:136-171`, transaction `:173-373`, enqueues `:374-392`);
  `attach/detach_pull_request` `utils/github_pr_links.py:44-174`;
  `get_actual_relation`/`get_inverse_relation`
  `utils/issue_relation_mapper.py:1-32`; `current_instance` api shape
  `api/serializers/issue.py:109+,:153-162,:436+` (PIDASHCONV-660 owns it).

## Handlers

- `handlers/FX-ISS-14.core.json` — `IssueViewSet.create` `:396-485`,
  `.retrieve` `:485-620`, `.partial_update` `:620-721`, PUT update via DRF
  default (`get_serializer_class` `:207-208`, routes
  `app/urls/issue.py:62-73`), `.destroy` `:721-755`,
  `BulkDeleteIssuesEndpoint` `:786-812`.
- `handlers/FX-ISS-15.reads.json` — user-properties `:756-784`,
  bulk-dates `:1119-1196`, meta `:1199-1211`, identifier `:1214-1489`,
  sub-issues `app/views/issue/sub_issue.py:33-248`.
- `handlers/FX-ISS-16.relations.json` — relations
  `app/views/issue/relation.py:37-284`, links
  `app/views/issue/link.py:26-113` (+ serializer `:801-850`), PR
  `app/views/issue/github_pr.py:28-73`, code-review
  `app/views/issue/git_code_review.py:24-74`.
- `handlers/FX-ISS-17.engage.json` — history
  `app/views/issue/activity.py:24-86`, comments
  `app/views/issue/comment.py:36-258`, issue reactions
  `app/views/issue/reaction.py:25-85`, subscribers
  `app/views/issue/subscriber.py:16-104`.
- `handlers/FX-ISS-18.labels_attachments.json` — labels
  `app/views/issue/label.py:23-117` (+ `LabelSerializer.validate_name`
  `app/serializers/issue.py:563-574`), attachments
  `app/views/issue/attachment.py:31-229`; cache
  `utils/cache.py` (`invalidate_cache` on `/api/workspaces/:slug/labels/`).
- `handlers/FX-ISS-19.archive.json` — archive/retrieve/unarchive
  `app/views/issue/archive.py:106-303`, bulk-archive `:306-344`;
  `CLOSED_STATE_GROUPS` `utils/constants.py:76-88`, `INVALID_ARCHIVE_*`
  `utils/error_codes.py:7`.
- `handlers/FX-ISS-20.versions_move.json` — versions
  `app/views/issue/version.py:27-144`, move
  `app/views/issue/move.py:17-46` (routes `app/urls/issue.py:279-298` +
  `:310-313`).

## Guards (signals + tasks)

- `guards/FX-ISS-21.signals_tasks.json` — orchestration receivers
  `orchestration/signals.py:61-107`, git_sync receivers
  `bgtasks/github_signals.py:31-74`, delete paths `db/mixins.py:49-82`;
  `issue_activity` `bgtasks/issue_activities_task.py:1503-1515`,
  `model_activity` `bgtasks/webhook_task.py:463-464`,
  `recent_visited_task` `bgtasks/recent_visited_task.py:17-18`,
  `issue_description_version_task`
  `bgtasks/issue_description_version_task.py:43-44`,
  `crawl_work_item_link_title` `bgtasks/work_item_link_task.py:262-263`,
  `get_asset_object_metadata` `bgtasks/storage_metadata_task.py:14-15`.

## Pilot-2 list anchors — verified at Ported from (not re-recorded)

Zero diff Ported-from→HEAD on all D-26 sources. Every `list/TRACE.md`
anchor re-grepped 2026-10-02 and holds: `base.py:85` IssueListEndpoint,
`:90` get, `:200` IssueViewSet, `:210` get_queryset, `:218`
apply_annotations (TRACE cites `:217` — 1-line slack, the `return` above),
`:258` list, `:813` DeletedIssuesListViewSet, `:829` IssuePaginatedViewSet,
`:988` IssueDetailEndpoint, `:1041` get + Exists `:1044-1071`;
`serializers/issue.py:105` Flat, `:1116` ListDetail;
`db/models/issue.py:229` issue_objects; `utils/paginator.py:642`
get_per_page, `:654` paginate; `filter_backend.py:20`,
`filterset.py:124`, `issue_filters.py:18`. `list/` untouched by this issue.

## Reuse audit (merged ports vs D-26 needs)

Space serializers (`services/src/space/serializers/issue_graph.rs` +
`taxonomy.rs`), ticker models (`db/src/tasks_ticker/models.rs`),
`sanitize_html` (`api/src/space/sanitize.rs`). Verdict per port — COVERS
(call it) or LACKS (the needing layer run extends the merged module in
place, naming both paths; never a parallel copy):

- COVERS: `issue_state_flat`, `issue_project_lite`, `issue_relation` +
  `related_issue`, `issue_cycle_detail`, `issue_module_detail`,
  `issue_link` (+dup check), `issue_attachment`, `issue_reaction`,
  `issue_flat`, `comment_reaction` (full), `comment_reaction_lite`,
  `issue_vote`, `cycle_to_representation`, `module_to_representation`,
  `label_to_representation`, `label_lite_to_representation`,
  ticker struct + `effective_max_ticks`/`remaining`/`cap_reached`/
  `pool_size_or_default`/`resolve_project_interval`/`tick_count`,
  `sanitize_html` (+`strip_tags`).
- LACKS (extend in place):
  - `issue_to_representation` ports `space/serializer/issue.py:164-201`
    (14 nests + all columns), NOT the app 29-key read shape
    (`app/serializers/issue.py:1039-1113`, cycle/assigned_pod/module/label/
    assignee/counts/is_synced keys). 639 extends the merged module.
  - `issue_comment_to_representation` has NO `is_synced` and nests
    `CommentReactionLite` where the app nests the full
    `CommentReactionSerializer` (`app/serializers/issue.py:953,956`).
    654 extends the merged module.
  - `issue_public_to_representation` is space's own twin — 639 ports the
    app `IssuePublicSerializer` (`:1415-1440`) as-is alongside it.
  - `_issue_is_actively_synced` / `_comment_is_actively_synced` have no
    services-usable port (D-32's copy is api-private) — 642 ports them
    into `serializers_engage.rs` (not a dup).
  - `classify_run_error` has no port — 639 ports
    `runner/diagnostics.py:173-244` into services (D-32's api-private
    copy is unusable from services).

## Ported bugs (translate as-is; each layer PR re-lists the ones it ports)

1. `IssueCreateSerializer.to_representation` echoes `initial_data` ids,
   not DB rows (`:185-191`).
2. Assignee/label project filters silently drop invalid ids (`:338-354`).
3. `create` swallows m2m `IntegrityError` (partial success, `:402-460`).
4. `update` deletes all m2m rows on explicit `[]` but keeps them when the
   key is absent (`:474-514`).
5. Missing serializer context keys are an unhandled `KeyError` 500.
6. `IssueVersionDetailSerializer` lists `name` twice (`:1459`+`:1476`).
7. Retrieve/archive-retrieve omit `is_intake` (unannotated → SkipField,
   35 keys) while identifier renders it (36 keys).
8. RETRACTED in review (2026-10-02): retrieve's subqueries DO emit
   `deleted_at IS NULL` via the default `SoftDeletionManager`
   (`db/mixins.py:53-54,66-67`; none of the four link models overrides
   `.objects`). Siblings' explicit guards are behaviorally redundant —
   same rows. Port KEEPS the guard (pilot-2 `annotation_selects`
   already emits it).
9. History default branch 500s (`instance["created_at"]` on models,
   `activity.py:81-84`; pinned by `test_history_default_500s`).
10. Lite `is_synced` is git-only (`base.py:1303-1309` + `:1238`).
11. Version paginate: malformed cursor → ValueError 500; size-0 cursor →
    ZeroDivisionError 500 (`global_paginator.py:33-51`).
12. `recent_visited_task` gets mixed int/str args per caller
    (`base.py:609-615` vs `:1476-1482`).
13. PUT update is undecorated (guests can PUT; PATCH is ADMIN/MEMBER).
14. `remove_relation` on unknown pair crashes on `None.delete()`
    (`relation.py:270-272`).
15. Bulk-archive enqueues activities for earlier issues before a later
    one 400s (`archive.py:320-339`).
16. Bulk label colors can be 7 hex digits (`randint` inclusive bound,
    `label.py:102`); bulk-create skips cache invalidation.
17. Label rename pre-check is case-sensitive but `validate_name` is
    iexact — two error shapes for one conflict (`label.py:61-70` vs
    `serializers/issue.py:563-574`).
18. V1 attachment delete destroys the S3 object then soft-deletes the
    row; v2 delete only flags (`attachment.py:71-72` vs `:150-153`).
19. V1 attachment list has no entity/uploaded filter; v2 does
    (`:89` vs `:191-197`).
20. Comment-create guest refusal is 400, not 403 (`comment.py:86-89`).
21. Issue-reaction dupes 400 via generic `handle_exception`
    (`{"error": "The payload is not valid"}`), unlike comment-reaction's
    specific message.
22. Sub-issue POST `current_instance` holds the sub id, not the old
    parent (`sub_issue.py:231`).
23. Subscriber list returns the member roster, not subscribers
    (`subscriber.py:52-57`).
24. Destroy DOES send pre/post_save (via `SoftDeleteModel.delete` →
    `save()`); both receiver pairs no-op on unchanged state — the "no
    signal" reading would be wrong (`mixins.py:72-78`).
