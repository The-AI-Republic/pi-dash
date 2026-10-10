# TRACE — D-37 ops fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`, deploy files `apps/api/`. Ported-from:
`01a93e17216faea7bfc156b0f864cbbe420d1c52`
(no drift — `git diff 01a93e17 -- <all D-37 paths below>` empty at record time;
the domain gate owns drift, not this issue).

Stdout/stderr/exit conventions (Django management): `self.stdout.write` →
stdout + newline; `self.stderr.write` → stderr; uncaught `CommandError(msg)` →
stderr `Error: {msg}`, exit 1; `print()` → stdout; `input(prompt)` → prompt to
stdout, reads stdin; `self.style.*` adds ANSI color only on a tty — fixtures
record the plain strings. Deviations (stdout errors, exit-0 failures) are
recorded per command and flagged BUGS where they look unintentional.

## Commands

- `commands/boot.golden.json` — F37-01 — `db/management/commands/wait_for_db.py:1-24` (poll loop :14-24, "waititng" typo :21, dead retry branch) and `db/management/commands/wait_for_migrations.py:1-26` (10s sleep :18, MigrationExecutor plan :22-26, success line :20).
- `commands/cache_storage.golden.json` — F37-02 — `db/management/commands/clear_cache.py:1-30` (--key :13-22 vs full clear :24-26, error line :29); `db/management/commands/create_bucket.py:1-61` (boto3 env wiring :20-27, head_bucket branches :32-58, int(Code) :37); `db/management/commands/update_bucket.py:1-187` (permission probes :29-100 incl. policy-probe side effect :80-98, make_objects_public :123-132, generate_bucket_policy :102-121, handle flow :134-187, permissions.json fallback :177-187).
- `commands/users.golden.json` + `users_sql.sql` — F37-03 — `db/management/commands/activate_user.py:1-38` ("does not exists" :32, is_active write :35-36); `db/management/commands/reset_password.py:1-66` (getpass prompts :43-44, mismatch/blank stderr :47-54, zxcvbn score<3 :56-59, set_password + is_password_autoset=False :62-64); `db/management/commands/create_instance_admin.py:1-43` (role=20 get_or_create :35, print(e) + re-raise :41-43); `db/management/commands/create_project_member.py:1-72` (role None-vs-20 :37, workspace guard :51-52, upsert :55-62, ProjectUserProperty :65, stdout error catch :70-72).
- `commands/dummy_data.golden.json` — F37-04 — `db/management/commands/create_dummy_data.py:1-74` (input() sequence :18-55, validations :21-30, Workspace/Member writes :36-45, synchronous task call :57-68, queue message :70) + call-args shape from `bgtasks/dummy_data_task.py:487-553` (helpers :44-484 incl. cycles off-by-one :153, parent no-op :378-388).
- `commands/repair_sql.sql` + `commands/repair.rows.json` — F37-05 — `db/management/commands/copy_issue_comment_to_description.py:1-53` (batch-500 loop :18-26, Description mapping :29-42, bulk_create/link :44-51); `db/management/commands/fix_duplicate_sequences.py:1-95` (identifier parse :41-47, strict_str_to_int :22-25, pg_advisory_xact_lock :59-66, renumber :68-91) + lock-key fn `utils/uuid.py:19-26`; `db/management/commands/update_deleted_workspace_slug.py:1-71` (all_objects :31 + `db/mixins.py:67`, guards :36-50, dry-run :58-59, slug__epoch write :61-69, mutated-slug-twice message :67).
- `commands/version_sync.golden.json` — F37-06 — `db/management/commands/sync_issue_version.py:1-21` (prompts :16-17, batch_size-as-str .delay :19) + task `bgtasks/issue_version_sync.py:234-235`; `db/management/commands/sync_issue_description_version.py:1-23` (prompts :18-19, .delay :21) + task `bgtasks/issue_description_version_sync.py:123-124`.
- `commands/instance.golden.json` — F37-07 — `license/management/commands/configure_instance.py:1-170` (SECRET_KEY gate :39-43, _is_db_sourced skip :23-29 + :46-47, get_or_create loop :48-59, DERIVED_FLAG_KEYS :20 + :61-170) + key table `utils/instance_config_variables/core.py:8-263` + `extended.py:5` + resolver `license/utils/instance_value.py:28-54` + registry `config/registry.py:36-307`; `license/management/commands/register_instance.py:1-92` (version fallback chain :28-38, GitHub fetch timeout=10 :40-51, create :67-77 vs update :82-87, instance_traces.delay :90) + task `license/bgtasks/tracer.py:26-27` + model `license/models/instance.py:18-30`.
- `commands/mail_pods_dryrun.golden.json` — F37-08 — `db/management/commands/test_email.py:1-67` (resolver order :27-35, get_connection "1"-string flags + timeout=30 :37-45, subject + template :47-50) + `license/utils/instance_value.py:57-74`; `runner/management/commands/ensure_project_pods.py:1-83` (missing scan :42-45, --dry-run :57-59, `{identifier}_pod_1` + get_or_create defaults :66-79); `db/management/commands/dry_run_scheduler_migration.py:1-238` (cron probe :71-80 + no-op notice :99-103, row shape :108-141, croniter-absent :50-68, 60s MATCH :176-190, --json envelope :194-210, human lines :212-238) + `bgtasks/_rrule.py:46,125,269`.
- `commands/prompting.golden.json` — F37-09 — `prompting/management/commands/reseed_default_template.py:1-26` (:24-26), `prompting/management/commands/reseed_review_template.py:1-26` (:24-26), `prompting/management/commands/reseed_test_template.py:1-26` (:24-26) + seed fns `prompting/seed.py:200-308` (names :29/:99 + `prompting/models.py:20`, created/refreshed/skipped :228-308) + `prompting/fragments/__init__.py` (glob + assemble); `prompting/management/commands/revalidate_section_overrides.py:1-68` (--clear :27-34, registry-miss rule :40-41, set/clear writes :54-64, summary :66-68) + `prompting/validation.py:323`.

## Seeds

- `seeds/loader.golden.json` — F37-10 — `bgtasks/workspace_seed_task.py:49-68` (SEED_DIR/data join :59, FileNotFoundError→None :63-65, JSONDecodeError→None :66-68, logger :46); `settings/common.py:743` (SEED_DIR) + `:27` (BASE_DIR); `seeds/data/*.json` (projects 1, states 8, labels 2, issues 7, pages 2, cycles 2, modules 3, views 1; uniform required keys verified per file).

## Routing

- `routing/decision_table.golden.json` — F37-11 — `middleware/db_routing.py:1-164` (READ_ONLY_METHODS :38, non-read→primary :56-59, process_view :70-96, attr order func→view_class→cls :115-144, None→primary :110-111, try/finally :61-68 + process_exception :146-164); `utils/core/request_scope.py:16-76` (set :28-44, should :47-59, clear :62-76, Local backend :25); `utils/core/dbrouters.py:21-75` (db_for_read :30-44, db_for_write :46-58, allow_migrate :60-75); `utils/core/mixins/view.py:24` (ReadReplicaControlMixin default True).

## Image

- `image/entrypoints.golden.json` — F37-12 — `apps/api/Dockerfile.api:1-58` (python:3.12.10-alpine :1, ENV :4-7, apk sets :10-39, pip install :35, COPY set :21-49, EXPOSE 8000 :56, CMD :58); `apps/api/bin/docker-entrypoint-api.sh:1-38` (wait_for_db :3 → wait_for_migrations :5 → sha256 signature :11-21 → register_instance :24 → configure_instance :27 → create_bucket :30 → clear_cache :33 → collectstatic :36 → gunicorn exec :38); `-api-local.sh:1-38` (same boot + DJANGO_SETTINGS_MODULE default :37 + uvicorn exec :38); `-migrator.sh:1-6` (wait_for_db $1 :4 → migrate $1 :6); `-worker.sh:1-33` (waits :4-6, min(nproc,8) :19-28, celery exec :33); `-worker-local.sh:1-11` + `-beat-local.sh:1-11` (watchmedo); `-beat.sh:1-8` (celery beat, no exec :8).
