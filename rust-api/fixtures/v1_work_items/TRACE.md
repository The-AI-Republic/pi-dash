# TRACE — D-18 api-v1 work-items fixtures (PIDASHCONV-659)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from (drift baseline): `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(zero diff Ported-from→HEAD on all sources below, verified 2026-10-02).

Method (all files): every value produced by running the named Python code —
Django shell probes for serializers/models/queries/guards/tasks shapes, live
HTTP (httpx, member `X-Api-Key`) against Django runserver for handler goldens,
permission matrix and task triggers, Celery protocol-v2 messages drained from
the redis broker for task payloads, `information_schema` for column lists.
Environment: `pi_dash.settings.test` on scratch Postgres `pidash_conv659`
(127.0.0.1:55438), `BASE_URL` http://127.0.0.1:18359, redis db 15,
`AMQP_URL=redis://127.0.0.1:6379/15`, `USE_MINIO=1` + test `AWS_*` (presign is
local boto3 signing; no S3 round-trip). Django 4.2.30, DRF 3.15.2.
IDs/datetimes/cursors/signatures are live values (volatile); key sets, key
order, statuses, SQL shapes and error bodies are the pins.

Consumers (layer sub-issues under PIDASHCONV-55): 660←F18-01;
661←F18-02+F18-03(LabelLite); 662←F18-02(links); 663←F18-02(relations);
664←F18-03(comments/attachments/activities); 665←F18-03(expand/search);
666←F18-04; 667←F18-05; 668←F18-06; 669←F18-07; 670←F18-08; 671←F18-09;
672←F18-10; 673–680←F18-11 (+ their layer fixtures); 677 also consumes the
move-SQL file. Gate: PIDASHCONV-76 (`rust-api/contract-tests/v1_work_items/`,
49 tests, 5 files).

## Serializers

- `serializers/F18-01.issue_serializer.golden.json` — `_same_uuid`
  `api/serializers/issue.py:54-67`, `normalize_description_input` `:68-108`,
  `IssueSerializer` `:109-496` (declared `:118-151`, `Meta` `:153-159`,
  `to_internal_value` `:177-180`, `validate` `:182-288` incl. assigned_pod
  `:190-216`, `create` `:290-371`, `update` `:373-427`, `get_url` `:429-434`,
  `to_representation` `:436-494`); `issue_web_url`/`web_base_url`
  `utils/host.py:70-97`; `markdown_to_html` `utils/markdown_converter.py:444-470`;
  `validate_html_content`/`validate_binary_data` `utils/content_validator.py:29-60,:211-241`;
  `relations_summary` `orchestration/blockers.py`; `grouped_relations`
  `orchestration/relations.py`; `has_active_run` `db/models/issue.py:232-250`;
  `Pod` `runner/models.py:52-160`.
- `serializers/F18-02.label_link_relation.golden.json` — `IssueLiteSerializer`
  `:497-508`, `IssueWorkpadSerializer` `:511-523`,
  `LabelCreateUpdateSerializer` `:526-554`, `LabelSerializer` `:557-577`,
  `IssueLinkCreateSerializer` `:580-620` (validate_url `:602-616`, dup guard
  `:617-620`), `IssueLinkUpdateSerializer` `:623-646`,
  `IssueLinkSerializer` `:649-669`, `GithubPullRequestLinkSerializer`
  `:672-699`, `GitCodeReviewLinkSerializer` `:700-728`,
  `IssueRelationResponseSerializer` `:729-769`,
  `IssueRelationCreateSerializer` `:770-807` (validate_issues `:801-807`),
  `IssueRelationRemoveSerializer` `:808-820`, `IssueRelationSerializer`
  `:821-861`, `RelatedIssueSerializer` `:862-906`; link save kwargs
  `api/views/issue.py:1633`.
- `serializers/F18-03.comment_attachment_activity_expand_search.golden.json` —
  `IssueAttachmentSerializer` `:907-925`, `IssueCommentCreateSerializer`
  `:928-962`, `IssueCommentSerializer` `:965-1019` (get_url `:994-999`,
  to_representation `:1001-1006`, validate `:1008-1017`),
  `IssueActivitySerializer` `:1020-1030`, `CycleIssueSerializer` `:1033-1045`,
  `ModuleIssueSerializer` `:1047-1059`, `LabelLiteSerializer` `:1061-1072`
  (consumed by PIDASHCONV-661), `IssueExpandSerializer` `:1074-1117`
  (get_labels `:1090-1095`, get_assignees `:1097-1101`),
  `IssueAttachmentUploadSerializer` `:1119-1136`, `IssueSearchSerializer`
  `:1139-1153`, advanced-search family `:1155-1203`.
- `serializers/F18-04.page_shapes.golden.json` — `PageLiteSerializer`
  `api/serializers/page.py:27-47`, `PageDetailSerializer` `:50-69`,
  `PageWriteSerializer` `:72-93`, `PageCreateSerializer` `:96-99`,
  `PageUpdateSerializer` `:102-112`; `html_to_markdown`
  `utils/markdown_converter.py`.

## Models

- `models/F18-05.columns.json` — `Issue` (`issues`, 34 cols),
  `Label` (`labels`, 15), `IssueLink` (`issue_links`, 12),
  `IssueComment` (`issue_comments`, 24), `IssueActivity`
  (`issue_activities`, 20), `FileAsset` (`file_assets`, 24),
  `IssueRelation` (`issue_relations`, 11), `GithubPullRequestLink`
  (`github_pull_request_links`, 18), `GitCodeReviewLink`
  (`git_code_review_links`, 23), `Page` (`pages`, 26), `ProjectPage`
  (`project_pages`, 9) — column names + nullability as observed in
  `information_schema`, cross-checked against each model's `_meta`
  (zero mismatch). For verification against the db crate.

## Queries

- `queries/F18-06.list_detail.json` — `WorkspaceIssueAPIEndpoint.get_queryset`
  `api/views/issue.py:206-223` + get lookup `:250-260`,
  `IssueListCreateAPIEndpoint.get_queryset` `:283-300`,
  `work_item_list_filters` `:368-370` + `utils/issue_filters.py:485-654`,
  ordering `:361-437` (`STATE_GROUP_ORDER` `utils/constants.py:76`),
  `BasePaginator` envelope/cursor `:439-444` + `utils/paginator.py:635-694`,
  external_id lookup `:346-359`, `IssueDetailAPIEndpoint.get_queryset`
  `:552-569` + get lookup `:600-605`, put upsert `:639-767` (unrouted — no
  `put` in any `api/urls/*.py`; live PUT is 405), patch lookup `:796`,
  delete lookup + admin/creator check `:884-897`.
- `queries/F18-07.subresources.json` — label list/detail querysets
  `:1322-1336` (detail inherits, `:1440`), link list/detail `:1557-1569`,
  `:1662-1680`, comment list/detail with `is_member` Exists `:1805-1828`,
  `:1961-1984`, activity list/detail inline filters `:2156-2165`,
  `:2211-2224` (+ 404 body `:2227`), attachment list filter `:2438-2444`,
  relation grouped aggregation `:2971-3008` (real executed SQL via
  `CaptureQueriesContext`), `get_actual_relation`
  `utils/issue_relation_mapper.py:19-32` + reverse set `:3077` + refetch
  `:3111-3130`, workpad get queryset `:3269-3273` + get `:3275-3279` +
  patch row lock `:3307-3321`, PR-link querysets
  `api/views/github_pr.py:37-49,:86-95`, review-link querysets
  `api/views/git_code_review.py:32-44,:86-95`.
- `queries/F18-08.search_page.json` — `IssueSearchEndpoint`
  `api/views/issue.py:2682-2720`, `IssueAdvancedSearchEndpoint`
  `:2779-2915` (manual result dicts `:2880-2910`, NOT the F18-03
  serializers), `issue_search_queryset`/`extract_snippet`/`_build_search_filter`
  `search/issue.py:36-199`, FTS parity pins vs
  `rust-api/crates/services/src/app_views_search/fts.rs`
  (`ISSUE_SEARCH_VECTOR` == `issues_fts_idx` `db/models/issue.py:261-264`),
  page `get_queryset`/`validate_parent`/`get_page_or_error`/`detail_response`
  `api/views/page.py:180-230`. (Label querysets live in F18-07, owned by
  PIDASHCONV-669 — not recorded twice.)

## Guards

- `guards/F18-09.guards.json` — `user_has_issue_permission`
  `api/views/issue.py:175-189`, `run_belongs_to` `:1070-1085`,
  `resolve_moved_by_run` `:1086-1122`, `_active_run_of_caller` `:1123-1132`,
  `_refuse_agent_action` `:1133-1161` + `_refuse_agent_retick` `:1162-1164`,
  per-endpoint role matrix (`ProjectEntityPermission`/
  `ProjectLitePermission`/`ProjectMemberPermission`/`IsAuthenticated`
  `app/permissions/project.py:56-143` as declared per class in
  `api/views/issue.py`, `api/views/page.py:177`, `api/views/github_pr.py`,
  `api/views/git_code_review.py`), page archive guard
  `api/views/page.py:491-517`.

## Tasks

- `tasks/F18-10.tasks.json` — `issue_activity.delay` call sites
  (`issue.activity.created` `:519`, `.updated` `:683,:843`,
  `.deleted` `:900`, `link.activity.created` `:1641`, `.updated` `:1750`,
  `.deleted` `:1783`, `comment.activity.created` `:1926`,
  `issue_relation.activity.created` `:3096`,
  `attachment.activity.created` `:2631`, `.deleted` `:2493`),
  `model_activity` (webhook) `:530,:693,:751,:853`, `post :511-516`
  created_at/created_by stamp, `crawl_work_item_link_title.delay` `:1634`,
  `get_asset_object_metadata.delay` `:2507,:2649`,
  `page_transaction.delay` + `track_page_version.delay` via
  `_record_body_write` `api/views/page.py:156-170`, S3 upload/confirm flow
  `:2235-2450` + `S3Storage` `settings/storage.py:19-101`; ambient
  `process_logs` per-request message and `soft_delete_related_objects` on
  issue delete recorded as observed. Celery v2 wire bytes per call site
  (raw + decoded task/args/kwargs).

## Handlers

- `handlers/F18-11.work_items.json` — one recorded request → status + body
  for the 38 `api/urls/work_item.py` routes incl. error bodies and
  deprecated `issues/` prefix twins (byte-identical, verified live).
- `handlers/F18-11.labels_pages.json` — the 2 `api/urls/label.py` + 3
  `api/urls/page.py` routes. `POST /pages/` is 503 here (no live-document
  server: `LIVE_URL` unconfigured); serializer 400s run first and are
  pinned; body-write tasks live in F18-10.
- `handlers/F18-11.move.json` — the 46 SQL statements issued by
  `move_work_item_to_project` `utils/issue_move.py:122-393` plus DB
  before/after (consumed by PIDASHCONV-677).

## Converters (PIDASHCONV-679)

- `converters/html_to_markdown.json` — `body_html → body_markdown`
  vectors from the live `markdownify()` 1.1.0 call behind the page body
  paths (`api/views/page.py`, bs4 4.12.3 parser).
- `converters/markdown_to_html.json` — `body_markdown → body_html`
  vectors from the live `TiptapHTMLRenderer` +
  `_resolve_task_lists` (`utils/markdown_converter.py:232,275`,
  markdown-it-py 4.2.0 + mdit-py-plugins 0.6.1) behind
  `render_body_html` (`api/views/page.py:121`).
- Method (both files): `{"id","input","output"}` records generated by
  running the named Python code; consumed by the `*_vectors` unit tests
  in `crates/api/src/v1_work_items/handlers_labels_pages.rs`
  (`include_str!`), which must match byte for byte.

## Ported bugs / behaviour notes (translate, don't redesign)

- `description_binary` is a read-only DRF `ModelField`: input is silently
  dropped and `validate()` `:240-243` (`validate_binary_data`) is
  unreachable via `IssueSerializer` (F18-01).
- `validate_issues` custom message is shadowed by `min_length=1`
  (F18-02); link-create input `issue_id` never reaches `validated_data` —
  the view's save kwarg is the sole source (F18-02).
- `CycleIssueSerializer`/`ModuleIssueSerializer` lack `Meta.model`: every
  render raises `AssertionError` (F18-03 records declared shape + nested
  key sets for the port).
- `PUT` on the detail route is unmapped (405); `put()` `:639-767` is dead
  via HTTP (F18-06).
- The lxml round-trip in `IssueSerializer.validate` drops a single 10MB
  text node (`tostring` → `<p></p>`); only multi-element >10MB HTML
  reaches `{"error": "html content is not valid"}` (F18-01).
- `""` `description_html` yields `"Invalid HTML passed"` (F18-01, F18-03).
- Duplicate `sequence_id`s make the by-identifier route 500
  (`MultipleObjectsReturned`, `:256`); deprecated twins return the
  identical bytes, 500 included (F18-11).
- `_refuse_agent_action` ignores run ownership: any active run on the
  issue named by the header refuses (F18-09).
- Advanced search builds result dicts manually: `state` is always
  `{"name","group"}` (None-filled when stateless), never null (F18-08).
- Relation pair uniqueness is one row per ordered pair
  (`issue_relation_unique_issue_related_issue_when_deleted_at_null`);
  the grouped read only knows stored types (F18-07).
- Comment search widens matches without an access filter (known
  limitation, `search/issue.py:78-83`; F18-08).
