# TRACE — D-29 app views + search fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
Read method: source-verbatim (shapes, key sets, SQL structure) rendered offline;
no DB connection on this runner, so result rows are shape-derived examples
whose keys are pinned by `rust-api/contract-tests/app_views_search/`
(`GLOBAL_VIEW_KEYS`, `VIEW_ISSUE_KEYS`, `PAGINATED_KEYS`,
`GLOBAL_RESULT_ENTITIES`, `PROJECT_SEARCH_KEYS`) and SQL is compiler-form.
Pages read: Porting Guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
(updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook (2026-09-29).

- `FX-VIEW-CRUD.json` — `app/views/view/base.py:52-135`
  (WorkspaceViewViewSet: `:56-58` perform_create, `:60-69` get_queryset,
  `:71-78` list, `:80-100` partial_update, `:102-112` retrieve,
  `:114-135` destroy) and `:256-398` (IssueViewViewSet: `:260-261`
  perform_create, `:263-287` get_queryset, `:289-306` list, `:308-341`
  retrieve, `:343-363` partial_update, `:365-398` destroy); routes
  `app/urls/views.py:13-57` (7 view routes); response key set
  `app/serializers/view.py:56-69` corroborated by
  `contract-tests/app_views_search/test_global_views.py:GLOBAL_VIEW_KEYS`.
- `FX-VIEW-ISSUES.sql` — `app/views/view/base.py:138-253`
  (WorkspaceViewIssuesViewSet: `:142-162` permission Q, `:164-210`
  apply_annotations, `:212-213` get_queryset, `:215-253` list pipeline);
  manager exclusions `db/models/issue.py:95-104`; envelope keys
  `utils/paginator.py:642-694` corroborated by `PAGINATED_KEYS`; row keys
  `app/serializers/view.py:14-53` corroborated by `VIEW_ISSUE_KEYS`.
- `FX-FAV.json` — `app/views/view/base.py:401-433`
  (IssueViewFavoriteViewSet: `:404-411` get_queryset, `:413-421` create,
  `:423-433` destroy); shapes `app/serializers/favorite.py:40-43,59-89`.
- `FX-FTS-CORE.sql` — `search/issue.py:1-207` (`:36-45` int4 guard,
  `:55-63` vector exprs, `:66-90` comment subquery, `:93-125` filter OR-chain,
  `:128-186` issue_search_queryset, `:189-198` extract_snippet, `:201-207`
  search_issues); index exprs `db/models/issue.py:255-266`
  (issues_fts_idx) and `:654-662` (issue_comments_fts_idx).
- `FX-GLOBAL-SEARCH.json` — `app/views/search/base.py:43-286`
  (`:54-65` workspaces, `:67-84` projects, `:86-109` issues, `:111-133`
  cycles, `:135-157` modules, `:159-203` pages, `:205-227` views,
  `:229-253` intakes, `:255-286` get + MODELS_MAPPER); route
  `app/urls/search.py:13-15`.
- `FX-ENTITY-SEARCH.json` — `app/views/search/base.py:289-691`
  (`:293-301` params, `:302-501` project branch: user_mention `:304-351`,
  project `:353-370`, issue `:372-394`, cycle `:396-442`, module `:444-471`,
  page `:473-500`; `:503-691` workspace branch: user_mention `:505-544`,
  project `:546-563`, issue `:565-586`, cycle `:588-633`, module `:635-661`,
  page `:663-690`); route `app/urls/search.py:23-25`.
- `FX-ISSUE-SEARCH.sql` — `app/views/search/issue.py:1-166`
  (`:24-31` project filter, `:33-40` query, `:42-50` parent exclusion,
  `:52-70` relation exclusion, `:72-81` root-only, `:83-88` cycle exclusion,
  `:90-95` module exclusion, `:97-102` target-date, `:104-166` get pipeline
  incl. `:146-149` guest scoping and `:151-164` values shape corroborated by
  `PROJECT_SEARCH_KEYS`); route `app/urls/search.py:18-20`.
- `FX-SER.json` — `app/serializers/view.py:1-86` (`:14-53`
  ViewIssueListSerializer, `:56-86` IssueViewSerializer);
  `app/serializers/favorite.py:1-89` (`:40-43` ViewFavoriteSerializer,
  `:46-56` entity map, `:59-89` UserFavoriteSerializer);
  `app/serializers/base.py:12-24` (DynamicBaseSerializer fields/expand quirk).
- `FX-MODEL.json` — `db/models/view.py:58-99` (IssueView + `:14-55`
  defaults + `:79-95` save); `db/models/favorite.py:14-69` (UserFavorite +
  `:52-64` save); `db/models/issue.py:95-104` (IssueManager),
  `:107-199` (Issue FTS-reference columns), `:255-266` (issues_fts_idx);
  `db/models/issue.py:550-666` (IssueComment + comment_stripped save
  `:599-606` + `:654-662` index); audit columns `db/mixins.py:16-44`
  (created_at/updated_at/created_by/updated_by), `db/models/base.py:17-22`
  (id PK), `db/mixins.py:60-70` (deleted_at/soft managers),
  `db/models/workspace.py:185-195` (workspace/project FKs).
- `FX-GUARD.json` — `app/permissions/base.py:13-109` (ROLE values,
  creator bypass, WORKSPACE vs PROJECT + admin fallback, 403 body) applied
  to the 10 gates in `app/views/view/base.py:71,80,114,216,289,308,343,365,413,423`
  plus undecorated actions (`:102` retrieve, `:404-411` fav list) and in-body
  rechecks (`:75-76,89,118-134,293-303,317-331,368-392`).
- `FX-TASK.json` — call sites `app/views/view/base.py:105-111` (project_id
  None) and `:334-340`; task body `bgtasks/recent_visited_task.py:17-62`;
  table `db/models/recent_visit.py:22-36`.
