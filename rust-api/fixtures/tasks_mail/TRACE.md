# TRACE — D-07 bgtasks mail + notifications fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/bgtasks/`. Templates under `apps/api/templates/emails/`.
Supporting modules noted where behaviour lives outside `bgtasks/`.
Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52` (drift baseline; the domain gate owns drift).

Method tags: `executed` = bytes produced by running the real code (probe in
`/tmp`, kept out of the repo per contributor rules); `source-derived` = DB /
Redis / SMTP-touching paths transcribed verbatim from source, covered live by
the oracle suite `rust-api/contract-tests/tasks_mail/test_mail_tasks.py`
(PIDASHCONV-21 worker plane). Pure helpers that need only stdlib/bs4 were
executed via AST extraction of the real function bodies
(`/tmp/probe_d07_pure.py`); Django/celery module imports are not executed, the
recorded defs are byte-identical to source.

Live scope: 11 tasks + 14 helpers = 25 units. `project_invitation_task.py` is
dead code (see `dead/`): no fixture, no test, Python stays in place.

## Wire — F-WIRE-MAIL

- `wire/F-WIRE-MAIL.wire.json` — executed. All 11 live tasks: `email_notification_task.py:47,153`
  (`stack_email_notification`, `send_email_notification`), `notification_task.py:191`
  (`notifications`), `magic_link_code_task.py:23`, `forgot_password_task.py:23`,
  `user_activation_email_task.py:23`, `user_deactivation_email_task.py:23`,
  `user_email_update_task.py:22,67`, `project_add_user_email_task.py:25`,
  `workspace_invitation_task.py:23`; bare-`@shared_task` options verified by AST over all 10
  files (no kwargs anywhere); beat entry `apps/api/pi_dash/celery.py:29-32`
  (`check-every-five-minutes-to-send-email-notifications`, `crontab(minute="*/5")`);
  wire bytes via real kombu `create_task_message` (celery 5.6.3, memory broker) +
  harness `assert_wire_message` per task; `eta_sample` (countdown=60) proves the
  F-09 `+00:00` rendering; `cpython_argsrepr` is CPython `repr()` of the passed args.

## Stack — F-STACK

- `stack/F-STACK.stack.json` — source-derived. `email_notification_task.py:46-84`:
  input query `:49`, receiver set `:55`, per-receiver grouping `:60-72`, fan-out
  `:75-81`, `processed_at` update `:84`. PORT BUG 1 (`email_notification_ids`
  spans the receiver's issues, `:72` vs `:80`) recorded verbatim. Model columns
  `db/models/notification.py:121-148`.

## Send — F-SEND

- `send/F-SEND.create_payload.golden.json` — executed. `email_notification_task.py:87-127`;
  PORT BUG 2 (`data.get("actor_id", {})` literal-key check, `:120`, last-wins proven).
- `send/F-SEND.remove_unwanted_characters.golden.json` — executed.
  `email_notification_task.py:27-30` (`[\x00-\x1F\x7F-\x9F]` strip).
- `send/F-SEND.send_email_notification.json` — source-derived. Lock helpers `:33-43`;
  mention/html helpers `:130-150`; task `:152-306` (lock-id format `:154-157`, redis
  `base_api` lookup `:164`, early return `:167-168` with PORT NOTE lock leak, payload
  build `:170`, per-actor pops `:189-221`, subject `:243`, context `:244-261`, SMTP
  send `:265-284`, `sent_at` update `:287`, exception branches `:292-306`). Template
  PORT NOTE (`current_site`/`total_updates`/`total_comments` never sent) proven by the
  rendered golden below.
- `send/F-SEND.issue-updates.rendered.json` — executed (Django 4.2.30
  `render_to_string`, task-shaped context per `:244-261`).
  `templates/emails/notifications/issue-updates.html`; plain text via real
  `utils/email.py:generate_plain_text_from_html`.

## Notifications — F-NOTIF

- `notifications/F-NOTIF.mention_helpers.golden.json` — executed.
  `notification_task.py:53-80` (`get_new_mentions`, `get_removed_mentions`),
  `:115-129` (`extract_mentions`), `:133-154` (`extract_comment_mentions`,
  `get_new_comment_mentions`).
- `notifications/F-NOTIF.notifications_task.json` — source-derived.
  `notification_task.py:37-50` (`update_mentions_for_issue`), `:84-111`
  (`extract_mentions_as_subscribers`), `:157-187` (`create_mention_notification`),
  `:190-674` (task: input coercion `:202-204`, 13 early-path types `:205-219`, member
  gate `:231-247`, subscriber query `:280-309`, sender branches `:311-317`, preference
  rules `:331-349`, bulk shapes `:364-450`, mention loops `:465-659` with PORT BUG 3
  (`receiver_id=subscriber` stale loop var, `:573-576` and `:622-625`), writes
  `:454-459,:661-670`, `print(e)` handler `:672-674`). Preference columns
  `db/models/notification.py:77-118`; `Notification` columns `:14-62`.

## Auth — F-AUTH

- `auth/F-AUTH.tasks.json` — source-derived subjects/contexts (no DB reads in any task):
  `magic_link_code_task.py:23-64` (PORT NOTE: `key` arg unused),
  `forgot_password_task.py:23-72` (abs_url shape `:26-27`),
  `user_email_update_task.py:22-65,67-114`.
- `auth/F-AUTH.rendered.json` — executed (Django 4.2.30, task-shaped contexts):
  `templates/emails/auth/magic_signin.html` (serves two tasks),
  `templates/emails/auth/forgot_password.html`, `templates/emails/user/email_updated.html`.

## Membership — F-MEMBER

- `membership/F-MEMBER.tasks.json` — source-derived SELECTs/subjects/URLs:
  `user_activation_email_task.py:23-69` (subject fallback chain `:27`, profile_url `:29`),
  `user_deactivation_email_task.py:23-71` (login_url `:29`),
  `project_add_user_email_task.py:25-89` (reads `:28-33`, project_url `:34`, fixed subject `:52`),
  `workspace_invitation_task.py:23-89` (email-keyed inviter lookup `:26`, invite lookup `:29`,
  relative_link `:32-34`, abs_url `:37`, subject `:49`, `invite.message` write `:58-59`,
  silent `DoesNotExist` return `:84-85` — note `User.DoesNotExist` NOT covered).
- `membership/F-MEMBER.rendered.json` — executed (Django 4.2.30, task-shaped contexts):
  `templates/emails/user/user_activation.html`, `templates/emails/user/user_deactivation.html`,
  `templates/emails/notifications/project_addition.html`,
  `templates/emails/invitations/workspace_invitation.html`.

## Common — F-COMMON

- `common/F-COMMON.email_configuration.json` — source-derived (EMAIL_* are db-sourced,
  live resolution needs DB): `license/utils/instance_value.py:28-74` (resolver),
  `:57-74` (7 keys + call-time `os.environ.get` defaults), `config/registry.py:82-88`
  (db source + decrypt flag), SMTP `== "1"` string-compare mapping used identically in
  all 11 live sends.
- `common/F-COMMON.exception_handling.json` — source-derived control-flow table:
  stack has no handler (`email_notification_task.py:46-84` — propagates);
  send branches `:292-306`; notifications `print(e)` (`notification_task.py:672-674`);
  broad `log_exception` + return in the 7 single-purpose mail tasks; invitation-task
  `DoesNotExist` silent returns (`workspace_invitation_task.py:84-85`,
  `project_invitation_task.py:82-83`); `log_exception` body
  (`utils/exception_logger.py:12-23`, always returns None).

## Dead — no fixture (SKIP, stays in place)

- `dead/DEAD.project_invitation.json` — evidence record only, NOT a fixture:
  `bgtasks/project_invitation_task.py:24` (`project_invitation`); (1) zero imports of the
  module under `apps/api` (grep this run); (2) only apparent call site
  `app/views/project/invite.py:104-105` calls `.delay()` on the `bulk_create` list
  (`:98-99`), never the task (loop var `invitation` unused); (3) no `celery.py` beat
  entry (grep `invitation`: no match); (4) worker-plane CI 2026-09-24: 74 registered
  tasks, this one missing. Oracle `MAIL_TASKS` excludes it by design. No fixture, no test.
