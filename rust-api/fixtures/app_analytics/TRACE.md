# TRACE — D-35 app analytics + exporters fixtures (PIDASHCONV-316)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Django pinned per `apps/api/requirements/base.txt`.
Ported-from (drift baseline, domain gate owns drift after this): `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
Gate: PIDASHCONV-93 (`rust-api/contract-tests/app_analytics/`).

## Serializers

- `serializers/analytic_serializers.golden.json` — FX-A-SER-01 `AnalyticViewSerializer`
  `app/serializers/analytic.py:10-31` (Meta `:11-14`, create `:16-22`, update `:24-31`).
- `serializers/exporter_serializers.golden.json` — FX-A-SER-02 `ExporterHistorySerializer`
  `app/serializers/exporter.py:11-30`.
- `serializers/importer_serializers.golden.json` — FX-A-SER-03 `ImporterSerializer`
  `app/serializers/importer.py:13-20` (model only, no handler).
- `serializers/porter_issue_serializers.golden.json` — FX-A-SER-04 `IssueExportSerializer` + get_*
  `utils/porters/serializers/issue.py:12-146` (fields `:36-67`, methods `:69-146`).

## Models

- `models/analytic_models.golden.json` — FX-A-MOD-01 `AnalyticView`
  `db/models/analytic.py:14-31` (table `analytic_views`, ordering `-created_at`).
- `models/exporter_models.golden.json` — FX-A-MOD-02 `ExporterHistory`
  `db/models/exporter.py:21-67` (table `exporters`, `generate_token` `:20-21`, ArrayField `:35`).
- `models/importer_models.golden.json` — FX-A-MOD-03 `Importer`
  `db/models/importer.py:13-36` (table `importers`, via `ProjectBaseModel` in `db/models/project.py:302-311`).

## Queries

- `queries/analytics_queries_part1.sql` — FX-A-Q-01 `AnalyticsEndpoint`
  `app/views/analytic/base.py:38-176` + `utils/analytics_plot.py:43-120` (plot builder);
  FX-A-Q-02 viewset queryset `base.py:177-189`, `SavedAnalyticEndpoint` `base.py:190-222`,
  `ExportAnalyticsEndpoint` `base.py:223-251`.
- `queries/analytics_queries_part2.sql` — FX-A-Q-03 `DefaultAnalyticsEndpoint`
  `base.py:252-390` + `ProjectStatsEndpoint` `base.py:391-455`.
- `queries/analytics_queries_part3.sql` — FX-A-Q-04 workspace advance
  `app/views/analytic/advance.py:32-351` + `utils/date_utils.py:125+` (filter ranges)
  + `utils/build_chart.py` (custom-work-items axes).
- `queries/analytics_queries_part4.sql` — FX-A-Q-05 project advance
  `app/views/analytic/project_analytics.py:32-367`; FX-A-Q-06 exporter queryset + filters
  `app/views/exporter/base.py:18-84`.

## Guards

- `guards/analytics_guards.golden.json` — FX-A-G-01 role x endpoint for all 14 routes
  (`app/permissions/base.py:13-60`, view decorators in `app/views/analytic/base.py`,
  `advance.py`, `project_analytics.py`, `app/views/exporter/base.py:22,67`);
  bodies + tripwires pinned by `contract-tests/app_analytics/test_permissions.py`.

## Tasks

- `tasks/export_tasks.golden.json` — FX-A-T-01 `issue_export_task`
  `bgtasks/export_task.py:128-226`; FX-A-T-02 `create_zip_file` `:28-38` +
  `upload_to_s3` `:42-124`. (`exporter_expired_task.py`, `analytic_plot_export.py` are D-09.)

## Formats

- `formats/porter_formats.golden.json` — FX-A-FMT-01 porter formatters
  `utils/porters/formatters.py:25-274`, `utils/porters/exporter.py:9-107`;
  `utils/exporters/exporter.py:12-76`, `utils/exporters/formatters.py:16-206`,
  `utils/exporters/schemas/issue.py:70-213` (+ `base.py`), `utils/csv_utils.py`.

## Handlers

- `handlers/analytics_handlers.golden.json` — FX-A-H-01 request/response per route
  (13 analytic in `app/urls/analytic.py:24-90` + export-issues GET/POST in
  `app/urls/exporter.py:11-15`); values are the PIDASHCONV-93 seed pins from
  `contract-tests/app_analytics/test_analytic_shapes.py` + `test_exporter.py`.

## Cross-cutting ported bugs (translate as-is, listed in the PR)

- `AnalyticViewSerializer.update` reads `query_data` (create reads `query_dict`) and
  line 30 unconditionally overwrites `query` with `issue_filters(...,"PATCH")` — dead if/else.
- `ExporterHistory.workspace` FK string is `"db.WorkSpace"` (capital S); `AnalyticView` uses `"db.Workspace"`.
- `DefaultAnalyticsEndpoint` aggregates `Sum("point")` (`base.py:371-372`).
- Advance intake differs: overview uses `issue_intake__status__in=[...]` (`advance.py:118-122`),
  `project_chart` uses `issue_intake__isnull=False` (`advance.py:222-224`).
- `get_filtered_counts` returns `{"count"}` only (previous-window line commented out, `advance.py:64`).
- Project chart cycle/module branch replaces the queryset with the through-table id list and
  counts daily buckets via `created_at__date` + `Q(issue__state__group=...)` (`project_analytics.py:223-258`).
- Project stats `get_work_items_stats` uses `Count(..., distinct=True)`; workspace stats do not.
- Porter `get_labels` reads `label_issue` through rows (soft-delete aware) while
  `IssueExportSchema.prepare_labels` reads `labels.all()` unguarded.
- Schema `relations` is a dict keyed by type — duplicate relation types overwrite.

## Production / review time per fixture (run 1, same-session blocks)

Hand-written against the sources above (no Django test client, no
record-and-freeze — each golden was composed from the cited lines, then
re-read against them; handler/guard values cross-checked against the
PIDASHCONV-93 suite, which is green against Django).

| Fixture | Production | Review | Reviewer pass |
|---|---|---|---|
| serializers (4 files) | ~25 min | ~12 min | re-read vs analytic.py, exporter.py, importer.py, porters issue.py |
| models (3 files) | ~10 min | ~5 min | columns vs model files; FK strings verbatim |
| queries (4 files) | ~40 min | ~20 min | ORM call re-read per section; seed rows vs contract tests |
| guards | ~10 min | ~5 min | decorators vs test_permissions.py tripwires |
| tasks | ~12 min | ~6 min | steps vs export_task.py lines |
| formats | ~15 min | ~8 min | defaults/branches vs formatter sources |
| handlers | ~15 min | ~8 min | pairs vs test_analytic_shapes.py + test_exporter.py |
| TRACE.md (this file) | ~10 min | ~5 min | every cited line re-grepped before commit |
| Total | ~137 min production | ~69 min review | |
