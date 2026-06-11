# 03 — Frontend Design (apps/web)

Stack facts this design follows: React Router pages under `app/`, MobX stores + SWR, Axios
`APIService` subclasses, Tailwind + `@pi-dash/ui`, existing SSE chat consumption pattern in
`core/components/runners/chat/use-agent-chat-events.ts` and the runner chat page
(`app/(all)/[workspaceSlug]/runners/chat/[runnerId]/page.tsx`).

## 1. Placement: chat on the landing page

The authenticated landing page is the workspace home:
`app/(all)/[workspaceSlug]/(projects)/page.tsx` → `WorkspaceHomeView`.

Design: an **assistant panel embedded at the top of workspace home** plus a route for full
conversations.

- **Home embed (`AssistantHomeWidget`)**: a prominent input ("Ask Pi to do something…") with 3-4
  suggestion chips ("What's assigned to me?", "Create an issue…", "Summarize project X",
  "Start a coding run on …"). Submitting creates a thread (or reuses the most recent unarchived
  one — product knob, default: new thread) and navigates to the thread route with the first turn
  already streaming. Recent threads listed beneath the input (title + relative time, max 5).
- **Full page route**: `/:workspaceSlug/assistant/` (thread list + new chat) and
  `/:workspaceSlug/assistant/:threadId` (conversation). Mirrors the runner chat page layout:
  scrollable message list, streaming assistant bubble, input docked at bottom, cancel button while
  a turn is active.

Workspace scoping comes from the URL param via the existing `useWorkspace()` store pattern — the
chat is inherently per-workspace, matching the thread model.

**Guest exclusion (product decision):** the widget and the `/assistant/*` routes render only for
workspace role ≥ MEMBER (role from the current workspace membership in the store); guests see
neither. The backend independently enforces this (`403 role_not_allowed` on every assistant
endpoint, 02 §9.1) — the UI check is presentation, not security.

**Widget thread behavior (product decision):** each widget submission starts a **new thread**;
recent threads are listed under the widget for one-click continuation.

## 2. Shared chat kit (extraction refactor) + new code

Reality check: the runner chat UI is one 333-line page component
(`app/(all)/[workspaceSlug]/runners/chat/[runnerId]/page.tsx`) with message rendering, delta
accumulation (`assistantDeltaText`, `appendAssistantDelta`, `isRealtimeEvent`), composer, and
send/stop logic **inline**, interleaved with runner-specific concerns (warm-up effect,
runner-status `disabledReason`, runner-keyed session bootstrap). Only `use-agent-chat-events.ts`
(46 lines) is separable today.

Step 1 — extract a **shared chat kit** (`core/components/chat/`: `use-chat-events`,
`use-chat-stream`, `delta.ts`, `ChatMessageList`, `ChatComposer`) and refactor the runner chat
page to consume it, keeping its runner-specific shell (warm-up, status badge, session bootstrap).
Full extraction design — line-by-line split of the current page, component/hook APIs, shared
event contract, and behavior-preservation verification — is in
**[06-chat-ui-refactor.md](06-chat-ui-refactor.md)**. Ships as its own refactor-only PR before
any assistant UI.

Step 2 — assistant-specific code:

```
packages/services/src/assistant/assistant.service.ts   # in @pi-dash/services, beside RunnerService
                                                        # (the kit's URL builders then come from one
                                                        # package for both consumers)
core/store/assistant/                   # MobX: thread list per workspace + active thread id ONLY —
                                        # live transcript state is owned by the kit's useChatStream
                                        # (runner chat works the same way; keeps consumers symmetric)
core/components/assistant/
├── home-widget.tsx                     # landing-page embed
├── thread-list.tsx
├── chat-root.tsx                       # composes the shared kit
├── tool-activity.tsx                   # collapsed "🔧 Searched issues (12 results)" rows (renderMessage slot)
└── assistant-message.tsx               # markdown rendering via the EXISTING MarkdownRenderer
                                        # (core/components/ui/markdown-to-component.tsx, react-markdown
                                        # already a dep) — do not add a second markdown renderer
core/components/settings/profile/content/pages/ai-assistant/   # BYOK settings tab
```

What the assistant adds that runner chat doesn't have: tool-activity rows, markdown rendering,
missing-LLM-config setup card, suggestion chips, thread list. What it drops: warm-up, runner
status, approvals.

## 3. Conversation UX details

- **Streaming**: SSE `chat.event` envelope identical to runner chat; event kinds and payload
  schemas are the canonical list in 02-backend.md §8.3 (`turn_started`, `message_created`,
  `assistant_delta`, `message_completed`, `tool_call`, `tool_result`, `turn_completed`,
  `turn_cancelled`, `turn_failed`). The kit applies `assistant_delta` (payload shape matches what
  `delta.ts` parses today: `payload.params.delta`); all other kinds reach the assistant's
  `onLifecycleEvent`, which **upserts `payload.message` into the transcript keyed by message id**
  (tool rows and completed messages arrive fully serialized in the event — no mid-stream
  refetch), with an SWR refetch on terminal `turn_*` events to reconcile. Reconnect with
  `?after=<last EVENT seq>` (the event counter, not message seq — two independent counters,
  02 §1). SSE auth is the session cookie (`EventSource` + `withCredentials`, as runner chat does).
- **Home-widget send sequence** (race-free by construction): `POST threads/` →
  `POST threads/<id>/messages/` (202 returns the serialized user message + turn) → navigate to
  the thread route, seeding the transcript with that message → `GET messages/` +
  SSE subscribe with `after=0` (full replay makes the navigation/subscribe race a non-issue;
  delta events for finished turns are pruned server-side, so replay stays small).
- **Tool transparency**: every `tool_call`/`tool_result` renders as a compact activity row in the
  transcript ("Created issue PROJ-142 — _Fix login redirect_", linkified). The durable source is
  the message rows; live events are the optimistic same-shaped upsert. Since writes auto-execute,
  visibility is the safety mechanism.
- **Created-object links**: tool rows carry `payload.links` =
  `{type, workspace_slug, project_id, issue_id, url_path}` — UI deep-links without URL parsing.
- **Failure states** (error codes from 02 §9.3): `llm_config_missing` (422 on send, plus
  pre-check via `GET llm-config/` which always returns 200 with `has_api_key:false` when unset) →
  inline setup card; `provider_auth_failed` → "Your API key was rejected by <host>";
  timeout/cancel → marked bubble with **Retry = posts a new user message with the same content**
  (a new turn; never re-executes the failed one).
- **Empty/loading**: skeletons per existing conventions; suggestion chips on empty thread.

## 4. BYOK settings tab

New profile settings tab **AI Assistant** (`/settings/profile/ai-assistant`). Registration spans
**two packages** (review-verified): add `"ai-assistant"` to the `TProfileSettingsTabs` string
union in `packages/types/src/settings.ts:11`, then add entries to `PROFILE_SETTINGS`
(`packages/constants/src/settings/profile.ts:25`) and `GROUPED_PROFILE_SETTINGS`. Follows the
existing two-column settings layout.

Fields: provider kind (select: "OpenAI-compatible" / "Anthropic"), base URL (hidden for Anthropic,
placeholder `https://openrouter.ai/api/v1`), model name (free text + datalist of known-good
suggestions: `anthropic/claude-sonnet-4-6` via OpenRouter, `meta-llama/llama-3.3-70b-instruct`,
`qwen/qwen-2.5-72b-instruct`, `deepseek/deepseek-chat`…), API key (write-only password field;
shows `••••1234` when set), **Test connection** button (calls `/llm-config/test/`, shows
success/failure inline), delete config.

Copy note shown under the form: "Tool-calling quality varies by model. We recommend models with
native function-calling support." (links to docs page).

## 5. Attribution surfacing

- Comments created by the assistant render the existing speaker-label treatment (small
  "Pi Assistant" badge) — the fields already exist on `IssueComment`; the work is exposing
  `speaker_type`/`speaker_label` in the comment serializer + a badge in the comment component.
- Issues show a subtle "via assistant" badge in detail view when `created_via == "assistant"`.

## 6. Cloud gating hooks (see 04-cloud.md)

**MVP: no plan/billing gating at all** (BYOK-only product decision, 04-cloud.md §2). The
frontend reads nothing from billing; visibility = workspace role ≥ MEMBER, usability = BYOK
config present. Post-MVP, when quotas/platform keys arrive, the frontend will read an
`assistant` entitlement block from the cloud billing endpoint and treat a 404/absent endpoint
(OSS) as `{enabled: true, byok_required: true, messages_limit: null}` — adapting off that block,
never off hardcoded plan names.
