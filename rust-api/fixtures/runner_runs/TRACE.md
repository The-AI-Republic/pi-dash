# TRACE — D-15 runner: runs fixtures (FX-RUN-01..09)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/runner/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(no drift — `git diff 01a93e17 -- apps/api/pi_dash/runner/` empty at record time).

Every value in every file was produced by running the named Python code
(venv: Python 3.12 / Django 4.2.30 / DRF 3.15.2, `pi_dash.settings.test`,
scratch Postgres `pidash_549_fixtures`). Each file's `_note` names its probe
script and method. Cross-domain collaborators (orchestration scheduling,
cloud-agent admission, matcher drains, redis pubsub, runner token crypto) were
stubbed at the seam; the D-15 unit under record always ran real. Bugs found
while recording are called out in the fixture notes (decide 500, chat-daemon
no-auth 500).

## Types + pure

- `fx-run-01-types-pure.golden.json` — FX-RUN-01 — `runner/models.py:207-329`
  (AgentRunStatus :207-234, AgentRunTrigger :237-253 + HUMAN_TRIGGERS :259-266
  + AUTOMATIC_ISSUE_TRIGGERS :273 + run_is_human_triggered :276-278,
  RefusalCategory :281-292, ApprovalStatus :295-299, ApprovalKind :302-306,
  AgentChatSessionStatus :309-312, AgentChatMessageRole :315-319,
  AgentChatMessageStatus :322-328) + ToolCallStatus `models.py:1178-1184`;
  `runner/services/usage.py:1-173` (coerce_token :69-79, normalize_usage
  :129-148, merge_usage :151-162, flat_token_fields :165-173); `runner/
  diagnostics.py:1-245` (infer_agent_label :103-128, enrich_run_error :149-170,
  classify_run_error :173-245); consts `views/runs.py:36-37`,
  `views/run_endpoints.py:45` + `:491-503`, `views/chat.py:64`,
  `services/chat.py:40-45`, `tasks.py:208,219`.

## Models

- `fx-run-02-models-runs.golden.json` — FX-RUN-02 — `runner/models.py`
  PodManager :42-49, Pod :52-177 (clean :127-143, save :145-159,
  default_for_project :161-171, default_for_project_id :173-176, Meta :98-122),
  AgentRun :872-1160 (save :1104-1134, is_terminal :1136-1144, is_active
  :1146-1159, Meta :1037-1102, token pseudo-fields :1024-1026), AgentRunEvent
  :1162-1175, AgentRunToolCall :1187-1216, ApprovalRequest :1219-1246,
  RunMessageDedupe :788-808; `runner/fields.py:1-61` (db_type :56-58, pre_save
  :60-61).
- `fx-run-03-models-chat.golden.json` — FX-RUN-03 — `runner/models.py`
  AgentChatSession :1249-1307, AgentChatMessage :1310-1350, AgentChatEvent
  :1353-1388, AgentChatApprovalRequest :1391-1430, ChatMessageDedupe :1433-1451,
  RunnerLiveState read subset :1454-1527 (observed_run_id :1481, last_event_at
  :1482, approvals_pending :1493, usage :1497, llm_model :1498, updated_at
  :1500, token properties :1517-1527; write path is D-14).

## Guards

- `fx-run-04-guards.golden.json` — FX-RUN-04 — `runner/services/permissions.py:
  1-137` (runner_visible_to_user_q :40-54, machine :57-67, runner :70-80,
  filter_runs_usable_by_runner :83-123, can_manage_runner :126-137);
  `runner/views/runs.py` _can_view_run :87-118 + _can_cancel_run :120-122;
  `runner/services/chat.py` can_read :48-53, can_send :56-59, can_decide :62-67;
  `runner/views/chat.py` ChatSendThrottle :60-61 + get_throttles :269-272;
  `settings/common.py:93-97` (runner_chat_send 60/minute).

## Lifecycle

- `fx-run-05-lifecycle.golden.json` — FX-RUN-05 — `runner/services/
  agent_run_finalization.py:1-199` (merge_done_payload :30-45,
  finalize_agent_run :48-86, _publish_effects :89-103, apply_terminal_effects
  :105-199); `runner/services/run_lifecycle.py:1-463` (apply_run_paused
  :158-244, resume_unavailable :247-295, assign_rejected_busy :298-322,
  _post_failure_comment :341-399, finalize_run_terminal :402-463);
  `runner/services/scheduler_hook.py:1-42`.

## Chat service

- `fx-run-06-chat-service.golden.json` — FX-RUN-06 — `runner/services/chat.py:
  1-497` (drain :82-109, normalize_cwd :112-115, seq allocators :118-125,
  serialize_event :128-137, publish :140-147, append_locked :150-171,
  record_dedupe :180-190, enqueue message :193-224, enqueue warm :227-253,
  dispatch-failed :256-302, create/active/finalize/complete :305-400, sweeps
  :403-497).

## Tasks

- `fx-run-07-tasks.golden.json` — FX-RUN-07 — `runner/tasks.py:1-358` (all 11
  `runner.*` tasks :42-353); `celery.py` beat dict :27-134 + settings-backed
  registrar :166-184; `settings/common.py` defaults (:465-487, :520-525, :557).

## Handlers — web

- `fx-run-08-handlers-web.golden.json` — FX-RUN-08 — `runner/views/runs.py:1-695`
  (list :131-219, create :221-345, run_ai :347-388, comment_and_run :420-466,
  re-tick :482-523, detail :530-542, cancel :549-639, release-pin :656-695);
  `runner/views/approvals.py:1-105`; `runner/views/metrics.py:1-105`;
  `runner/signals.py:1-66` (`create_default_pod_for_new_project` :32-66 —
  "ensure-default-pod" in the issue is descriptive only); serializer shapes
  `runner/serializers.py` run :260-317 + approval :320-339.
- Ported bug (contract-pinned): web decide always 500s on Postgres
  (`select_for_update` over the nullable `agent_run__runner` join,
  approvals.py:58) — recorded as the raised `NotSupportedError`; the 404/409/
  flip/fan-out branches are unreachable live.

## Handlers — daemon (+ web chat + SSE)

- `fx-run-09-handlers-daemon.golden.json` — FX-RUN-09 — `runner/views/
  run_endpoints.py:1-514` (all 12 endpoints); `runner/views/chat.py:1-857`
  (7 daemon endpoints :513-778, SSE :781-857, plus the 8 web chat endpoints
  :131-510 — the split gave web chat no separate FX id, so its goldens live
  here for whole-file coverage); chat serializer shapes `runner/serializers.py`
  :342-423.
- Ported bug: daemon chat `_resolve` dereferences `auth_runner.id` with no
  None guard (chat.py:522) — unauthenticated daemon chat POSTs raise
  `AttributeError` (live 500), recorded from a live call.
