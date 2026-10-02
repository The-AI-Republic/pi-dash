-- queries/binding_sql.sql
-- Binding reads/writes record: SQL shape + write effects + next_run_at rules.
-- Source: app/views/scheduler/views.py:163-291. Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.
-- Method: str(queryset.query) under pi_dash.settings.test (Postgres dialect,
-- no DB round-trip); literals replaced with :placeholders below.
--
-- R1 list (:175-182): project + slug scope, select_related(scheduler,
--   last_run, pod), ORDER BY -created_at. Joins: workspaces INNER (filter),
--   schedulers INNER (non-null FK), agent_run LEFT (nullable last_run),
--   pod LEFT (nullable pod, NO deleted_at filter - a soft-deleted pod row
--   still joins and renders its stale pod_name).
-- R2 detail GET/PATCH-lookup/uninstall-lookup (:240-245,:255-260,:284-289):
--   pk + project + slug scope with NO select_related (workspaces join only);
--   the serializer's scheduler/last_run/pod dereferences fire one extra
--   query each (N+1 vs R1 - port the queries, not the count).
-- R3 install lookups (:192-199): project (pk + slug -> 404 'No Project ...'),
--   then scheduler guard (pk + SAME workspace + is_enabled=True -> 404
--   'No Scheduler ...' when disabled, foreign-workspace, or missing). The
--   guard runs BEFORE serializer validation, so a missing/invalid scheduler
--   id in the body 404s instead of 400ing.
-- R4 unique check (DRF UniqueTogetherValidator, see F36-03): EXISTS over the
--   double-deleted_at-NULL-filtered queryset for (scheduler, project).
-- R5 install INSERT (:207-212): validated fields + pinned scheduler/project/
--   workspace/actor; model defaults for omitted keys; created_by via crum.
-- R6 next_run_at recompute (views.py:216-220 vs :266-274 - the asymmetry is
--   INTENTIONAL, port it): the computation is forwarded to
--   bgtasks/scheduler.py:70-83 _next_fire_for_binding(binding, now) =
--   next_fire_from_rrule(dtstart, rrule or '', tzid or 'UTC',
--   coerce(rdates), coerce(exdates), now). INSTALL always computes after
--   save and writes back only when the value is non-null AND differs from
--   the stored value (:218-220; stored is NULL at that point so in practice
--   'when non-null'). PATCH recomputes only when request.data contains one
--   of (dtstart, rrule, rdates, exdates, tzid) (:266-268), refreshes from
--   DB first (:270), and writes back whenever non-null WITHOUT comparing
--   (:272-274). Both write via save(update_fields=['next_run_at',
--   'updated_at']). Rust seam: the recompute arrives as an injected
--   next_fire_for_binding closure (inputs: dtstart, rrule, tzid, raw
--   rdates/exdates JSON, now; output: Option<DateTime>); this file pins the
--   callers' I/O, not the expansion.
-- R7 PATCH save (:263): full-column UPDATE like F36-04 R4, then the
--   conditional R6 UPDATE.
-- R8 uninstall (:290): binding.delete() soft-delete (deleted_at + async
--   cascade task), 204 with empty body.

-- R1 binding list (:175-182)
SELECT "scheduler_bindings"."created_at", "scheduler_bindings"."updated_at", "scheduler_bindings"."created_by_id", "scheduler_bindings"."updated_by_id", "scheduler_bindings"."deleted_at", "scheduler_bindings"."id", "scheduler_bindings"."workspace_id", "scheduler_bindings"."project_id", "scheduler_bindings"."scheduler_id", "scheduler_bindings"."dtstart", "scheduler_bindings"."tzid", "scheduler_bindings"."rrule", "scheduler_bindings"."rdates", "scheduler_bindings"."exdates", "scheduler_bindings"."extra_context", "scheduler_bindings"."enabled", "scheduler_bindings"."outcome_mode", "scheduler_bindings"."next_run_at", "scheduler_bindings"."last_run_id", "scheduler_bindings"."last_error", "scheduler_bindings"."actor_id", "scheduler_bindings"."pod_id", "schedulers"."created_at", "schedulers"."updated_at", "schedulers"."created_by_id", "schedulers"."updated_by_id", "schedulers"."deleted_at", "schedulers"."id", "schedulers"."workspace_id", "schedulers"."slug", "schedulers"."name", "schedulers"."description", "schedulers"."prompt", "schedulers"."source", "schedulers"."is_enabled", "schedulers"."color", "agent_run"."id", "agent_run"."workspace_id", "agent_run"."owner_id", "agent_run"."created_by_id", "agent_run"."pod_id", "agent_run"."runner_id", "agent_run"."pinned_runner_id", "agent_run"."work_item_id", "agent_run"."scheduler_binding_id", "agent_run"."parent_run_id", "agent_run"."status", "agent_run"."executor_kind", "agent_run"."dispatch_attempts", "agent_run"."cancel_requested_at", "agent_run"."cancel_reason", "agent_run"."error_code", "agent_run"."tool_plan", "agent_run"."terminal_hooks_applied_at", "agent_run"."terminal_capacity_released_at", "agent_run"."prompt", "agent_run"."trigger", "agent_run"."prompt_manifest", "agent_run"."phase_kind", "agent_run"."run_config", "agent_run"."required_capabilities", "agent_run"."thread_id", "agent_run"."agent_metadata", "agent_run"."lease_expires_at", "agent_run"."done_payload", "agent_run"."error", "agent_run"."refusal_category", "agent_run"."llm_model", "agent_run"."usage", "agent_run"."input_tokens", "agent_run"."output_tokens", "agent_run"."total_tokens", "agent_run"."created_at", "agent_run"."assigned_at", "agent_run"."queue_position", "agent_run"."started_at", "agent_run"."ended_at", "pod"."id", "pod"."workspace_id", "pod"."project_id", "pod"."name", "pod"."description", "pod"."created_by_id", "pod"."is_default", "pod"."deleted_at", "pod"."created_at", "pod"."updated_at" FROM "scheduler_bindings" INNER JOIN "workspaces" ON ("scheduler_bindings"."workspace_id" = "workspaces"."id") INNER JOIN "schedulers" ON ("scheduler_bindings"."scheduler_id" = "schedulers"."id") LEFT OUTER JOIN "agent_run" ON ("scheduler_bindings"."last_run_id" = "agent_run"."id") LEFT OUTER JOIN "pod" ON ("scheduler_bindings"."pod_id" = "pod"."id") WHERE ("scheduler_bindings"."deleted_at" IS NULL AND "scheduler_bindings"."project_id" = :project_id AND "workspaces"."slug" = :slug) ORDER BY "scheduler_bindings"."created_at" DESC;

-- R2 binding detail lookup (:240-245; same shape :255-260, :284-289)
SELECT "scheduler_bindings"."created_at", "scheduler_bindings"."updated_at", "scheduler_bindings"."created_by_id", "scheduler_bindings"."updated_by_id", "scheduler_bindings"."deleted_at", "scheduler_bindings"."id", "scheduler_bindings"."workspace_id", "scheduler_bindings"."project_id", "scheduler_bindings"."scheduler_id", "scheduler_bindings"."dtstart", "scheduler_bindings"."tzid", "scheduler_bindings"."rrule", "scheduler_bindings"."rdates", "scheduler_bindings"."exdates", "scheduler_bindings"."extra_context", "scheduler_bindings"."enabled", "scheduler_bindings"."outcome_mode", "scheduler_bindings"."next_run_at", "scheduler_bindings"."last_run_id", "scheduler_bindings"."last_error", "scheduler_bindings"."actor_id", "scheduler_bindings"."pod_id" FROM "scheduler_bindings" INNER JOIN "workspaces" ON ("scheduler_bindings"."workspace_id" = "workspaces"."id") WHERE ("scheduler_bindings"."deleted_at" IS NULL AND "scheduler_bindings"."id" = :binding_id AND "scheduler_bindings"."project_id" = :project_id AND "workspaces"."slug" = :slug) ORDER BY "scheduler_bindings"."created_at" DESC;

-- R3 install project lookup (:192)
SELECT "projects"."id", "projects"."workspace_id" FROM "projects" INNER JOIN "workspaces" ON ("projects"."workspace_id" = "workspaces"."id") WHERE ("projects"."deleted_at" IS NULL AND "projects"."id" = :project_id AND "workspaces"."slug" = :slug) ORDER BY "projects"."created_at" DESC;
-- (full SELECT * column list omitted; only id/workspace_id are read afterwards)

-- R3 install scheduler guard (:194-199)
SELECT "schedulers"."created_at", "schedulers"."updated_at", "schedulers"."created_by_id", "schedulers"."updated_by_id", "schedulers"."deleted_at", "schedulers"."id", "schedulers"."workspace_id", "schedulers"."slug", "schedulers"."name", "schedulers"."description", "schedulers"."prompt", "schedulers"."source", "schedulers"."is_enabled", "schedulers"."color" FROM "schedulers" WHERE ("schedulers"."deleted_at" IS NULL AND "schedulers"."is_enabled" AND "schedulers"."id" = :scheduler_id AND "schedulers"."workspace_id" = :workspace_id) ORDER BY "schedulers"."created_at" DESC;

-- R4 unique check (validator .exists()): SELECT 1 variant of the R1 WHERE core
SELECT 1 AS "a" FROM "scheduler_bindings" WHERE ("scheduler_bindings"."deleted_at" IS NULL AND "scheduler_bindings"."deleted_at" IS NULL AND "scheduler_bindings"."project_id" = :project_id AND "scheduler_bindings"."scheduler_id" = :scheduler_id) LIMIT 1;
-- on PATCH the self row is additionally excluded: AND NOT ("scheduler_bindings"."id" = :binding_id)

-- R5 install INSERT (:207-212): validated fields + pinned ids/actor; omitted keys take model defaults (tzid 'UTC', rrule '', rdates/exdates [], extra_context '', enabled true, outcome_mode 'create_issue', next_run_at NULL, last_run NULL, last_error '', pod NULL)
INSERT INTO "scheduler_bindings" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_id", "project_id", "scheduler_id", "dtstart", "tzid", "rrule", "rdates", "exdates", "extra_context", "enabled", "outcome_mode", "next_run_at", "last_run_id", "last_error", "actor_id", "pod_id") VALUES (:id, :now, :now, :user_id, NULL, NULL, :workspace_id, :project_id, :scheduler_id, :dtstart, :tzid, :rrule, :rdates, :exdates, :extra_context, :enabled, :outcome_mode, NULL, NULL, '', :actor_id, :pod_id);

-- R6 next_run_at write-back (:218-220 install / :272-274 patch)
UPDATE "scheduler_bindings" SET "next_run_at" = :computed, "updated_at" = :now WHERE "scheduler_bindings"."id" = :binding_id;

-- R7 PATCH save (:263): full-column UPDATE, then conditional R6
UPDATE "scheduler_bindings" SET "updated_at" = :now, "updated_by_id" = :user_id, "dtstart" = :dtstart, "tzid" = :tzid, "rrule" = :rrule, "rdates" = :rdates, "exdates" = :exdates, "extra_context" = :extra_context, "enabled" = :enabled, "outcome_mode" = :outcome_mode, "pod_id" = :pod_id WHERE "scheduler_bindings"."id" = :binding_id;

-- R8 uninstall (:290): soft-delete (updated_at/updated_by refreshed by save(); async cascade task also enqueued)
UPDATE "scheduler_bindings" SET "deleted_at" = :now, "updated_at" = :now2, "updated_by_id" = :user_id WHERE "scheduler_bindings"."id" = :binding_id;
