# TRACE — D-36 app: scheduler occurrences + bindings fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/app/`, models `apps/api/pi_dash/db/models/`, jobs
`apps/api/pi_dash/bgtasks/`, runner `apps/api/pi_dash/runner/`, utils
`apps/api/pi_dash/utils/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(no drift — `git diff 01a93e17 -- <all D-36 sources>` empty at record time).

Explicitly NOT recorded here: `db/models/scheduler.py` (all 251 lines) was
already ported by D-10 (PIDASHCONV-206, merged #590) with fixtures under
`rust-api/fixtures/tasks_ticker/models/` — no model column fixtures here; the
queries fixtures reference those tables only through SQL + rows.

## Serializers

- `serializers/scheduler_shapes.golden.json` — F36-01 — `app/serializers/scheduler.py:74-112` (SchedulerSerializer fields :79-92, read-only :93-100, active_binding_count :102-108, color :110-112 + `_validate_color` :37-43); annotation sites `app/views/scheduler/views.py:59-68,99-104,118-123`; id rule `app/serializers/base.py` (BaseSerializer).
- `serializers/binding_shapes.golden.json` — F36-02 — `app/serializers/scheduler.py:114-180` (derived fields :115-130, fields :134-160, read-only :161-173, last_run methods :175-179); pod queryset rule :123-127.
- `serializers/binding_validation.golden.json` — F36-03 — `app/serializers/scheduler.py:37-71,181-268` (`_validate_color` :37-43, `_validate_iso_datetime_list` :46-71, `validate_rrule` :181-198, rdates/exdates :200-204, tzid :206-216, extra_context :218-223, cross-field `validate` :225-268); rrule verdict `bgtasks/_rrule.py:218-265` (recorded as injected-closure I/O per the split-review seam note); error normalization is DRF 3.15.2 `as_serializer_error` (top-level list-wrap, nested-dict passthrough).

## Queries

- `queries/scheduler_sql.sql` + `.rows.json` — F36-04 — `app/views/scheduler/views.py:46-157` (list :55-72, create :75-85, detail GET :95-111, PATCH :114-133, DELETE cascade :136-157); fallback count `app/serializers/scheduler.py:108`; soft-delete `db/mixins.py:61-81`.
- `queries/binding_sql.sql` + `.rows.json` — F36-05 — `app/views/scheduler/views.py:163-291` (list :172-186, install :189-224, detail GET :237-249, PATCH :252-278, uninstall :281-291); forwarded computation `bgtasks/scheduler.py:70-83` (recorded as injected-closure I/O); unique validator is DRF 3.15.2 `get_unique_together_constraints` (conditional constraint yields pre-filtered queryset).
- `queries/pod_lastrun_columns.json` — F36-06 — `runner/models.py:52-95` (Pod :70-93 + Meta :98-122), `runner/models.py:872-~1040` (AgentRun incl. `scheduler_binding`, `status`, `started_at`, `ended_at`; Meta `db_table = "agent_run"`); cross-checked against `rust-api/fixtures/dispatch/fx-disp-02-models.golden.json` (status values identical).
- `queries/occurrences_window.golden.json` — F36-07 — `app/views/scheduler/occurrences.py:54-103` (caps :54-55, defaults :86-89, invalid_window :91-95, window_too_large :96-103); `utils/iso_datetime.py:19-57` (`parse_iso_utc` :19-33, `coerce_iso_datetimes` :36-57).
- `queries/occurrences_merge.golden.json` — F36-08 — `app/views/scheduler/occurrences.py:105-208` (future filter :109-119, expansion loop + cap :130-160, past SELECT :166-177, past rows :178-192, string sort :196, has_more/next_window_start :198-199, envelope :201-207); expansion call shape `bgtasks/_rrule.py:339-~355 occurrences_between` (recorded as injected-closure I/O).

## Guards

- `guards/permissions.golden.json` — F36-09 — `app/urls/scheduler.py:16-46` (5 routes :18-45); `app/permissions/base.py:19-84` (ROLE :13-17, WORKSPACE branch :44-51, PROJECT branch :52-78, denial :81-84); `app/views/scheduler/views.py:32-40` (`_feature_enabled`, `_disabled_response`); flag default `settings/common.py:446`; authN `app/views/base.py:189-194` (BaseAPIView); project-slug rewrite `app/views/base.py:48-81` + `db/models/project.py:192-220` (404 "Project not found"); 404/401 bodies are DRF 3.15.2 `exception_handler` (`{'detail': ...}`).

## Handlers

- `handlers/scheduler_io.golden.json` — F36-10 — `app/views/scheduler/views.py:46-157` (list :55-72, create :75-85 incl. IntegrityError path via `app/views/base.py:211-240`, detail GET :95-111, PATCH :114-133, DELETE :136-157); shapes F36-01, SQL F36-04, gates F36-09.
- `handlers/binding_io.golden.json` — F36-11 — `app/views/scheduler/views.py:163-291` (list :172-186, install :189-224, detail GET :237-249, PATCH :252-278, uninstall :281-291); shapes F36-02/F36-03, SQL F36-05, gates F36-09.
- `handlers/occurrences_io.golden.json` — F36-12 — `app/views/scheduler/occurrences.py:63-208` (project check :78, window :80-103, envelope :201-207); window F36-07, merge F36-08, gates F36-09.
