# TRACE — D-26 issue-list family fixtures (pilot 2, PIDASHCONV-11)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Django pinned per `apps/api/requirements/base.txt`.
Ported-from (rust-dev at branch time): `662031e9`.
Gate: the list subset of PIDASHCONV-84 (14 tests — every suite GET on
`issues/`, `issues/list/`, `v2/issues/`, `deleted-issues/`, `issues-detail/`;
enumerated in the issue workpad) passes byte-identical vs both backends.

## Handlers

- `handlers/list_family.golden.json` — flat `IssueListEndpoint.get`
  `app/views/issue/base.py:85-~198` (auth `:89`, 400 `:90-94`, base
  queryset `:~98`, visit side effect `:156`); paginated
  `IssueViewSet.list` `base.py:200-~320` (queryset `:210-216`, mismatch
  `:~320`); deleted `DeletedIssuesListViewSet.get` `base.py:813-826`;
  v2 `IssuePaginatedViewSet.list` `base.py:829-~987` (required_fields
  `:883-~910`); detail `IssueDetailEndpoint.get` `base.py:988-~1118`
  (Exists permission `:1044-~1071`); cutover routes
  `rust-api/crates/api/src/app_issues/mod.rs:80-101` + `overlay.rs:133-142`.

## Serializers

- `serializers/list_shapes.golden.json` — `IssueFlatSerializer`
  `app/serializers/issue.py:105-~124`; `IssueListDetailSerializer`
  `app/serializers/issue.py:1116-~1218`; v2 required_fields
  `base.py:883-~910`; envelope `BasePaginator.paginate`
  `utils/paginator.py:654-~694`; suite key sets
  `rust-api/contract-tests/app_issues/test_issue_core.py:12-31`.

## Queries

- `queries/list_queryset.sql` — manager `db/models/issue.py:229`
  (`issue_objects = IssueManager()`); annotations
  `base.py:217-~257` (`apply_annotations`); grouper array subqueries
  (assignee deleted-only, module deleted+archived, label deleted-only);
  tenant scoping `workspace__slug` + `project_id` on every path;
  deleted-issues OR `base.py:816-824`.

## Guards

- `guards/list_permissions.golden.json` — `app/permissions/` role gates
  (`ROLE.ADMIN/MEMBER/GUEST`, GUEST=5); guest `created_by` scoping vs the
  literal Exists (3 branches, `base.py:1044-~1071`); 401/403 bodies pinned
  by `test_permissions.py:25-27` (`FORBIDDEN`, `VIEWSET_FORBIDDEN`, `ANON`);
  24-path anon sweep `test_permissions.py:38-70`.

## Filters

- `filters/list_filters.golden.json` — `ComplexFilterBackend`
  `utils/filters/filter_backend.py:20`; `IssueFilterSet`
  `utils/filters/filterset.py:124`; legacy compiler
  `utils/issue_filters.py:18-~330` (helpers `:86-~330`); `order_by` /
  `updated_at__gt` param reads (`base.py:~881`, `:816-818`).

## Pagination

- `pagination/list_pages.golden.json` — `BasePaginator.get_per_page`
  `utils/paginator.py:642-652`; `paginate` `utils/paginator.py:654-~694`;
  cursor protocol `per_page:page:index`; group_by/sub_group_by verbatim
  params + mismatch 400 (`base.py:~320`).

## Cross-cutting ported bugs (translate as-is, listed in the PR)

- DynamicBaseSerializer `fields = self.expand` kills `fields=`; detail
  serializer ignores `fields` entirely.
- m2m-filter fanout: Django counts/dupes, Rust `COUNT(DISTINCT)`/dedupes.
- Module array: Rust adds `m.deleted_at IS NULL` Django lacks (fix queued).
- Detail link arrays: Django lists duplicate link rows, Rust dedupes.

## Production / review time per fixture (run 4, same-session blocks)

Hand-written against the sources above (no Django test client, no
record-and-freeze — each golden was composed from the cited lines, then
re-read against them).

| Fixture | Production | Review | Reviewer pass |
|---|---|---|---|
| handlers/list_family.golden.json | ~20 min | ~10 min | re-read vs base.py:85-100, 210-230, 813-830, 878-910, 1041-1075 |
| serializers/list_shapes.golden.json | ~15 min | ~8 min | key sets diffed vs suite LIST_KEYS/RETRIEVE_KEYS + shape.rs constants |
| queries/list_queryset.sql | ~25 min | ~12 min | subquery guards vs grouper notes; tenant scoping on all 4 paths |
| guards/list_permissions.golden.json | ~10 min | ~5 min | bodies vs test_permissions.py:25-27; 401 sweep list |
| filters/list_filters.golden.json | ~12 min | ~6 min | anchors vs filter_backend.py:20, filterset.py:124, issue_filters helpers |
| pagination/list_pages.golden.json | ~8 min | ~5 min | protocol vs paginator.py:642-694; mismatch body byte check |
| TRACE.md (this file) | ~15 min | ~5 min | every cited line re-grepped before commit |
| Total | ~105 min production | ~51 min review | |

Times are wall-clock blocks inside the single fixture run, rounded to the
minute-scale granularity the work actually took; review means a second
read-through against the cited source lines, not a re-run of the suite.
