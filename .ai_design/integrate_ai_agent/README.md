# Pi Dash AI Assistant — Design

End-to-end design for integrating a multi-tenant AI agent ("Assistant") natively into the
pi-dash cloud backend, serving all tenants from one stateless agent runtime.

## Documents

| Doc                                              | Contents                                                                                |
| ------------------------------------------------ | --------------------------------------------------------------------------------------- |
| [01-overview.md](01-overview.md)                 | Goals, requirements, product decisions, system architecture                             |
| [02-backend.md](02-backend.md)                   | Django app, models, agent runtime (Pydantic AI), tools, access control, streaming, BYOK |
| [03-frontend.md](03-frontend.md)                 | Chat box on landing page, SSE streaming UI, settings UI                                 |
| [04-cloud.md](04-cloud.md)                       | Private pi-dash cloud gating: plans, quotas, future platform keys                       |
| [05-rollout.md](05-rollout.md)                   | Phasing, testing, risks, open questions                                                 |
| [06-chat-ui-refactor.md](06-chat-ui-refactor.md) | Shared chat kit: extraction of runner chat UI into reusable components                  |

## Requirements (from product owner, 2026-06-11)

1. Code lives in the **OSS pi-dash repo**, cloud side (Django API + web app) — not in the local runner daemon.
2. Chat history stored in the **existing pi-dash Postgres DB**.
3. Chat box lives on the **landing page** (authenticated workspace home).
4. Agent has **tools to operate pi-dash on the user's behalf**: query projects/issues, create issues,
   comment, etc. — anything a user can do manually. Multi-turn **agent loop**, not fire-and-forget.
5. **Access control parity (critical):** the agent must see/do exactly what the requesting user could
   see/do by clicking through the UI. No privilege escalation, no cross-tenant leakage.
6. In private pi-dash cloud, **free users get the assistant from day 1 via BYOK**
   (own LLM API key + model + provider URL in user settings). Paid users later get
   platform-provided keys with no setup.

## Product decisions (confirmed)

| Decision           | Choice                                                                                        |
| ------------------ | --------------------------------------------------------------------------------------------- |
| Write actions      | **Auto-execute** (no confirm step); agent reports what it did                                 |
| Chat scope         | **Per-workspace threads** (multiple threads per user per workspace)                           |
| V1 powers          | **Dashboard create/read/update (no deletes) + dispatch coding AgentRuns**                     |
| Attribution        | **User-owned + "via assistant" marker** (reuse `speaker_type`/`speaker_label`)                |
| Monetization (MVP) | **BYOK-only, no plan quotas** — rate throttle is the only brake; quota design parked in 04 §2 |
| Guests             | **Hidden from guests** — UI renders for role ≥ MEMBER; backend 403 backstop                   |
| Home widget        | **New thread per submission**; recent threads listed for continuation                         |
| Thread deletion    | **Hard delete** (active turn cancelled first)                                                 |

## Core architecture in one paragraph

One **stateless agent runtime** (Pydantic AI, module-level definition, zero tenant state) serves all
tenants concurrently. Per request, the authenticated user's identity is packaged into a frozen
`AssistantDeps` object and injected into the run; every tool resolves data through the **same
queryset-scoping and role checks the DRF views use**, so authorization is enforced by the existing
permission layer, not by the prompt. Conversation history is rows in Postgres keyed by
`(workspace, user, thread)`. Turns execute in Celery; token deltas stream to the browser over the
existing SSE pattern (`chat.event`): sync Redis publish from the worker, async Redis subscribe in
the SSE view, persisted event rows for resume — the proven runner-chat delivery spine. The LLM is whatever the user configured
(BYOK: any OpenAI-compatible endpoint, or Anthropic), passed per-run — no per-user agent instances,
no per-user processes.
