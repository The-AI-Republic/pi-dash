# 01 — Overview & Architecture

## Goal

A built-in AI assistant in pi-dash cloud that any workspace member can chat with from the landing
page. The assistant operates pi-dash _as the user_ — querying and creating projects, issues,
comments, and dispatching coding agent runs — through a multi-turn agent loop, with strict
access-control parity and per-workspace chat history.

## What it is NOT

- Not another coding agent behind the Rust runner (`runner/` is untouched; the runner remains the
  executor for coding work — the assistant can _dispatch_ runs to it, like a user moving an issue
  into a ticking state).
- Not a per-user agent service or process. There are no agent "instances." One stateless runtime
  serves everyone.
- Not a separate identity. The assistant acts strictly as the authenticated user; objects it
  creates are owned by the user with a "via assistant" marker.

## System diagram

```
                       Browser (apps/web, React Router + MobX)
                       chat box on /:workspaceSlug landing page
                          │ POST message            ▲ SSE deltas (chat.event)
                          ▼                         │
   ┌────────────────── Django API (ASGI: gunicorn + UvicornWorker) ──────────────────┐
   │                                                                                 │
   │  DRF views: pi_dash.assistant (NEW OSS app)                                     │
   │   threads / messages / events(SSE) / llm-config                                 │
   │        │ enqueue turn                                  ▲ subscribe              │
   │        ▼                                               │                        │
   │  Celery worker ── run_assistant_turn(thread, msg) ── event rows + Redis pub/sub │
   │        │                                                                        │
   │        ▼                                                                        │
   │  ASSISTANT RUNTIME (stateless, module-level)                                    │
   │   Pydantic AI Agent ── per-run: model (from user BYOK config),                  │
   │                         deps=AssistantDeps(user, workspace, role),              │
   │                         message_history (loaded from Postgres)                  │
   │        │ tool calls                                                             │
   │        ▼                                                                        │
   │  TOOLS (pi_dash/assistant/tools/) ── reuse existing scoping + role checks       │
   │   query/create issues, comments, projects … dispatch AgentRun                   │
   │        │                                                                        │
   │        ▼                                                                        │
   │  Existing permission/service layer                                              │
   │   ProjectMember/WorkspaceMember queryset scoping (app/views, app/permissions)   │
   │   orchestration.service.handle_issue_state_transition()                         │
   └───────────────────────────────┬─────────────────────────────────────────────────┘
                                   │
                  Postgres (threads, messages, BYOK config)  +  Redis (pubsub, Celery)
                                   │
                  User's LLM endpoint (BYOK: OpenAI-compatible base_url / Anthropic)
```

## Why this shape (decisions already made in design discussion)

1. **Stateless shared runtime, per-request deps injection** — the same pattern as a Django view:
   defined once, tenant scope arrives with the request. Concurrent users are concurrent
   `agent.run()` calls (async, I/O-bound); no cross-talk because each run carries its own deps and
   its own loaded history. This is the only design that satisfies "different users talk to the
   assistant simultaneously without per-user agent instances."

2. **Pydantic AI as the loop library** — chosen over (a) embedding pi/claw-code (TypeScript,
   single-tenant application design, coding-agent toolset — wrong on all three axes), (b) a
   hand-rolled Anthropic loop (locks out open-source models), (c) a hand-rolled OpenAI-format loop
   (viable, but we'd rebuild schema generation, validation retry, streaming, and history
   serialization that Pydantic AI provides; its `deps_type`/`RunContext` is purpose-built for the
   tenancy boundary we need). Any OpenAI-compatible endpoint works via `OpenAIChatModel` +
   `OpenAIProvider(base_url=…)`, which is exactly the BYOK requirement.

3. **Session state externalized to Postgres** — Pydantic AI deliberately has no built-in session
   store; messages serialize to stable JSON (`ModelMessagesTypeAdapter`). We persist them in
   tenant-scoped rows, which is what makes sessions multi-tenant, durable across deploys, and
   horizontally scalable. (Requirement 2 satisfied for free.)

4. **Turns run in Celery, stream via Redis → SSE** — long multi-tool turns must survive
   `--max-requests` worker recycling and not pin ASGI workers. Reuse is deliberately scoped: the
   existing runner chat is HQ↔field transport (cloud relaying to agents on dev machines) and its
   dispatch half + runner-keyed models are **not** reused; but its delivery spine — persisted
   event rows with `seq`, Redis pub/sub publish, replay-then-subscribe SSE view
   (`runner/views/chat.py:769-845`), and the browser hook — is producer-agnostic and is copied as
   the template, fed in-process by the Celery loop instead of over HTTP by a runner.

5. **Access-control parity by construction** — tools never use raw unscoped ORM. Each tool funnels
   through shared helpers that apply the _same_ membership/role filters the views apply (see
   02-backend.md §5). The model literally cannot fetch what the user couldn't fetch.

## Key existing code this builds on

| Piece                                                    | Path                                                                                                    |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Role model (Admin 20 / Member 15 / Guest 5)              | `apps/api/pi_dash/db/models/workspace.py:19`                                                            |
| Workspace membership                                     | `apps/api/pi_dash/db/models/workspace.py:198-231`                                                       |
| Project membership + role decorator                      | `apps/api/pi_dash/app/permissions/base.py:19-88` (membership/role layer — separate from queryset layer) |
| Authenticated issue queryset (workspace/project scope)   | `apps/api/pi_dash/app/views/issue/base.py:199-215` (`IssueViewSet.get_queryset`)                        |
| Workspace role helpers (to be moved to a neutral module) | `apps/api/pi_dash/runner/services/permissions.py`                                                       |
| Representative scoped queryset (comments)                | `apps/api/pi_dash/app/views/issue/comment.py:43-46,68-75`                                               |
| Coding-run dispatch entry point                          | `apps/api/pi_dash/orchestration/service.py:99` (`handle_issue_state_transition`)                        |
| Comment attribution fields (`speaker_type`…)             | `apps/api/pi_dash/db/models/issue.py:519-556`                                                           |
| SSE chat precedent (API)                                 | `apps/api/pi_dash/runner/views/chat.py`                                                                 |
| SSE chat precedent (web hook)                            | `apps/web/core/components/runners/chat/use-agent-chat-events.ts`                                        |
| Chat page UI template                                    | `apps/web/app/(all)/[workspaceSlug]/runners/chat/[runnerId]/page.tsx`                                   |
| Landing page (auth'd)                                    | `apps/web/app/(all)/[workspaceSlug]/(projects)/page.tsx`                                                |
| Cloud plan/quota seams                                   | `private-pi-dash/pi_dash_cloud/billing/plans.py`, `quotas/middleware.py`                                |
