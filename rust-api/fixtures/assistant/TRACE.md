# TRACE — D-06 assistant + MCP + SSE fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/assistant/`. EE seams: `apps/api/pi_dash/ee/assistant/`.
Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

Vectors marked "executed" were produced by running the real Python offline
(project venv, `pi_dash.settings.test`, no DB connection): pure validators,
crypto round-trips, SSRF verdicts (numeric IPs need no DNS), title/content
helpers, `_classify_error`, envelopes, and compiler-rendered SQL. Settings,
codes, columns, URLs, and endpoint shapes are read verbatim from source.

## Serializers (F-A6-01)

- `serializers/thread.golden.json` — `assistant/serializers.py:20-29` (fields, read-only set, `get_has_active_turn`); no-message-serializer note `:32-34`.
- `serializers/llm_config.golden.json` — `assistant/serializers.py:37-75` (write-only api_key, validators, openai_compatible base_url gate).
- `serializers/stt_config.golden.json` — `assistant/serializers.py:78-120` (no provider_kind, base_url always required).
- `serializers/mcp_server.golden.json` — `assistant/serializers.py:123-162` (write-only auth_header max 2048, read-only tool_prefix, url/name validators).

## Errors (F-A6-02)

- `errors.json` — `assistant/errors.py:15-107` (base `:15-28`, 13 subclasses `:30-102`, limits `:105-107`).

## Models (F-A6-03)

- `models/columns.json` — `assistant/models.py:23-293`: ThreadKind `:23-25`, AssistantThread `:28-61`, TurnStatus `:64-69`, AssistantTurn `:72-105`, MessageKind `:108-113`, MessageStatus `:116-120`, AssistantMessage `:123-149`, AssistantEvent `:152-176`, ProviderKind `:179-181`, AssistantMCPServer `:184-235` (incl. `tool_prefix` `:226-235`, unique constraint `:214-216`), UserLLMConfig `:238-262`, UserSTTConfig `:265-293`.

## Crypto (F-A6-04)

- `crypto.json` — `assistant/crypto.py:37-233` (registry `:193-214`, Fernet `:134-190`, KMS `:57-131`, public API `:220-233`).

## SSRF (F-A6-05)

- `ssrf.json` — `assistant/ssrf.py:14-43` (`blocking_enabled` `:21-22`, `is_blocked` `:25-43`).

## Queries (F-A6-06)

- `queries.json` — `assistant/runtime/events.py:37-143` (channel, serialize, seq alloc, append, create, envelope, prune, publish) and `assistant/runtime/history.py:27-60` (load caps/ordering, dump); ORM SQL for `assistant/tasks.py:73-78,514-518`, `assistant/views/_base.py:31-34`, `assistant/views/messages.py:87,95`, `assistant/views/events.py:44-46`, `assistant/views/threads.py:60-63`, `assistant/runtime/llm.py:67-68`, `ee/assistant/stt_provider.py:66`, `assistant/runtime/mcp.py:250`; update shapes `assistant/tasks.py:99-107,157-213`.
- SQL is compiler-form (offline render; no DB credentials on this runner). Rows are shape-derived from the serialize/envelope key sets, not executed rows.

## Permissions (F-A6-07)

- `perms.json` — `assistant/views/_base.py:1-34` (role gate, owned_thread); `assistant/views/events.py:28-41` (SSE resolve matrix); throttle scopes `assistant/views/messages.py:29`, `assistant/views/llm_config.py:81,111`, `assistant/views/stt_config.py:84`, `assistant/views/transcribe.py:57`, `assistant/views/agent_profile.py:67`; rates `pi_dash/settings/common.py:93-117`; roles `pi_dash/core/permissions.py:23-25`.

## Runtime (F-A6-08)

- `runtime.json` — `assistant/runtime/llm.py:35-115` (key cache, gates, build branches, label); `assistant/runtime/title.py:31-147` (consts, clean/strip/content vectors, DeepSeek + Anthropic branches); `assistant/runtime/instructions.py:19-86` (BASE, loop appendix, dynamic); `assistant/runtime/deps.py:19-51`; `assistant/runtime/agent.py:21-29`; `assistant/runtime/markdown.py`.

## MCP + EE (F-A6-09)

- `mcp.json` — `assistant/runtime/mcp.py:37-309` (consts, settings, prefixes `:212-233`, auth header `:236-239`, toolsets `:242-296`, ResilientToolset `:73-172`); `ee/assistant/model_provider.py:45-134`; `ee/assistant/stt_provider.py:28-72`.

## Tools + tasks (F-A6-10)

- `tools-tasks.json` — `assistant/tools/_scoping.py:28-103`; `assistant/tools/_results.py:20-69`; `assistant/tools/issues.py:25-490` (9 `@assistant.tool` functions observed; the issue text says 8 — all 9 recorded verbatim); `assistant/tools/comments.py:21-`, `assistant/tools/github.py:35-`, `assistant/tools/projects.py:17-`, `assistant/tools/runs.py:18-`; `assistant/tasks.py:45-526` (limits, cancel key, streamer, run flow, classify, celery tasks).

## Endpoint shapes recorded inline (no separate fixture id)

- URLs: `assistant/urls.py:35-110` (15 paths under `workspaces/<slug>/ai-assistant` + `users/me/ai-assistant/`).
- Message POST gate order + 202 shape: `assistant/views/messages.py:60-115`; cancel: `:119-128`.
- Thread list (chat-only, 50, reap 1h empty): `assistant/views/threads.py:24-79`.
- SSE replay + frames + headers: `assistant/views/events.py:44-116`.
- MCP CRUD errors + effective prefix: `assistant/views/mcp_servers.py:28-152`.
- LLM/STT config CRUD + test shapes: `assistant/views/llm_config.py:30-143`, `assistant/views/stt_config.py`.
- Transcribe caps (25MB, formats, timeout): `assistant/views/transcribe.py:41-143`.
- Agent profile/token: `assistant/views/agent_profile.py:55-126`; CE seams in `ee/assistant/model_provider.py:71-108`.
