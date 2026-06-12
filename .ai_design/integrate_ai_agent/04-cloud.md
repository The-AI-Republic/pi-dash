# 04 — Private Pi-Dash Cloud Integration

Principle (matches existing architecture): **all assistant functionality lives in OSS pi-dash**;
`private-pi-dash` only layers plan gating, quotas, and (future) platform-provided LLM keys via its
existing seams — settings overrides in `pi_dash_cloud/settings/cloud.py` and, where file-level
override is needed, `ee-overlay/`.

## 1. Day-1 cloud behavior (free users, BYOK)

- Assistant available to **all plans including FREE**, requiring user BYOK config — this is pure
  OSS behavior; cloud needs no override to enable it.
- `ASSISTANT_ENCRYPTION_KEY` (comma-separated MultiFernet key list, 02-backend.md §7) added to
  SSM (`/pidash/<env>/assistant-encryption-key`) → env-file → settings, following the existing
  secret pipeline (ssm-bootstrap).
- `ASSISTANT_BLOCK_PRIVATE_URLS=True` in cloud settings (single SSRF policy defined in
  02-backend.md §7: loopback + link-local incl. 169.254.169.254 + RFC1918, checked at save, at
  test, and at connect time via custom httpx transport).

## 2. Quota & plan gating — DEFERRED POST-MVP (product decision 2026-06-11)

**MVP is BYOK-only with no plan quota.** Every cloud user (all tiers incl. FREE) brings their own
key and pays their own tokens; the only platform-compute brake is the OSS-level per-user rate
throttle on message POST (02-backend.md §8.1: default 30 messages/hour, settings knob — cloud may
tighten via env). No `Plan` dataclass change, no billing-endpoint change, no entitlement block,
and no frontend billing read ship in MVP. The `quota_exceeded` error code stays **reserved** in
the taxonomy (02 §9.3) so the UI path exists when quotas arrive.

Settled design kept for the post-MVP implementation (do not re-litigate then): plan field
`assistant_messages_per_month` (suggested 200/1k/5k/∞/∞, final numbers = product call at that
time); enforcement = atomic Redis `INCR` on `assistant:quota:<workspace_id>:<YYYYMM>` (EXPIRE
~40 days) in the message-create endpoint, post-INCR compare → `402 quota_exceeded`; no refunds
for failed turns; 409-rejected posts uncounted; nightly Postgres rollup; endpoint-level (the
`_check_hard_limits` middleware stub at `pi_dash_cloud/quotas/middleware.py:47-51` stays
untouched); frontend keys off an `assistant` entitlement block from the billing endpoint with
404-fallback defaults for OSS (03-frontend.md §6).

## 3. Future: platform-provided keys for paid users

When the home-page paid integration lands:

- New cloud-only model resolution hook via the ee/ce stub pattern (`ee-overlay/README.md`).
  **Reality check (review-verified): no Python-side `pi_dash/ee/` exists in OSS today** — only
  web packages have ee stubs. Therefore Phase 1 OSS work creates `pi_dash/ee/__init__.py` and
  `pi_dash/ee/assistant/model_provider.py` (re-exporting the CE BYOK-only
  `runtime/llm.py::resolve_model`), and `runtime/llm.py` callers import through that path.
  Cloud's `ee-overlay/apps/api/pi_dash/ee/assistant/model_provider.py` then replaces the stub at
  image build (`COPY ee-overlay/ /code/`, Dockerfile:50-51).
- Cloud implementation: if `plan.allows_feature("assistant_platform_keys")` → return a
  platform-managed model (platform OpenRouter/Anthropic key from SSM, per-plan model allowlist,
  per-tenant token metering for cost attribution into billing); else fall through to BYOK.
- Token metering: `result.usage()` per turn → usage rows by workspace/user → feeds billing.
  (Schema for this should be added in v1 already — `AssistantTurn.usage` covers it — so
  history exists when pricing decisions are made.)

## 4. Observability (cloud)

- Pydantic AI emits OpenTelemetry spans natively; wire to the existing logging/monitoring stack
  (no Logfire dependency required). Minimum v1: structured logs per turn (workspace, user, model,
  duration, tool calls count, token usage, outcome) — enough for abuse triage and cost forecasting.
- Provider error rates by `base_url` host surface BYOK-endpoint quality issues before users blame
  the product.
