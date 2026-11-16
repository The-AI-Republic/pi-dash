# TRACE — D-30 app: pages fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/app/`, models `apps/api/pi_dash/db/models/`, utils
`apps/api/pi_dash/utils/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(no drift — `git diff 01a93e17 -- <sources>` empty at record time).

## Serializers

- `serializers/page_serializers.golden.json` — F30-01 — `app/serializers/page.py:25-133` (PageSerializer field list :25-59, create :61-106, update :108-126, PageDetailSerializer :129-133); id rule `app/serializers/base.py` (BaseSerializer).
- `serializers/page_binary_update.golden.json` — F30-02 — `app/serializers/page.py:173-225` (fields :173-178, binary validate :180-198, html validate :200-211, update :213-225); validators `utils/content_validator.py` (validate_binary_data, validate_html_content, MAX_SIZE, SUSPICIOUS_BINARY_PATTERNS).
- `serializers/page_version_shapes.golden.json` — F30-03 — `app/serializers/page.py:136-171` (PageVersionSerializer :136-150, PageVersionDetailSerializer :153-170).

## Models

- `models/page_columns.json` — F30-04 — `db/models/page.py:23-77` (Page columns :30-58, Meta :60-64, save/stripped :70-77); `utils/html_processor.py` (strip_tags/MLStripper).
- `models/through_columns.json` — F30-05 — `db/models/page.py:80-182` (PageLog :80-117 incl. indexes :108-114 + unique_together :103; PageLabel :120-132; ProjectPage :135-155 incl. partial unique constraint :142-148; PageVersion :158-182 incl. save :175-182).

## Queries

- `queries/get_queryset.sql` + `.rows.json` — F30-06 — `app/views/page/base.py:81-127` (guards :91-98, is_favorite Exists :82-87 + :102, project Exists :107-110 + filter :125, ArrayAgg :111-124, distinct :126; double order_by :103 then :105).
- `queries/summary.sql` + `.rows.json` — F30-07 — `app/views/page/base.py:421-469` (queryset :422-438, guest scoping :441-451, aggregates :453-467); roles `app/permissions/base.py:13-17` (GUEST=5).
- `queries/archive_cte.sql` + `.rows.json` — F30-08 — `app/views/page/base.py:59-72` (CTE) + `:308-366` (archive :308-337, unarchive :339-366 incl. detach :360-362).

## Guards

- `guards/permissions.golden.json` — F30-09 — `app/permissions/page.py:18-125` (owner bypass :44-46, private-deny :48-50 + :80-85, role matrix :87-125) + inline guards `app/views/page/base.py` (partial_update lock :163-164, parent :166-173, access :176-180 + :281-285, retrieve guest :212-226 + missing :228-229, list guest :294-304, archive :317-326, destroy :376-394).

## Tasks

- `tasks/publish.golden.json` — F30-10 — `app/views/page/base.py` publish envelopes: `page_transaction.delay` (:144-148, :187-192, :560-565, :613-617), `track_page_version.delay` (:568-572, existing_instance JSON :552), `recent_visited_task.delay` (:237-243, track_visit default :205), `copy_s3_objects_of_description_and_assets.delay` (:620-626); codes `utils/error_codes.py:12-13` (PAGE_LOCKED=4701, PAGE_ARCHIVED=4702).

## Handlers

- `handlers/io.golden.json` — F30-11 — `app/views/page/base.py` per-action I/O: create :129-152, partial_update :154-200, retrieve :202-244, lock :246-256, unlock :258-269, access :271-289, list :291-306, archive :308-337, unarchive :339-366, destroy :368-419, favorites :472-495, description :498-575 (streaming headers :517-518), duplicate :578-639 (rename :597, binary reset :598, re-link :604-611, re-fetch :628-637); routes `app/urls/page.py` (11 routes).
- `handlers/versions.golden.json` — F30-12 — `app/views/page/version.py:19-31` (pk-branch :21-26, collection-branch :28-31); shapes `app/serializers/page.py:136-171`.
