# 05 — Rollout, Testing, Risks, Open Questions

## Phasing

**Phase 1 — Backend foundation (OSS)**

- `pi_dash.assistant` app with **app-local migrations**: `AssistantThread`, `AssistantTurn`,
  `AssistantMessage`, `AssistantEvent`, `UserLLMConfig`; plus a `db`-app migration for
  `Issue.created_via`. Crypto (MultiFernet + `assistant_reencrypt_llm_keys` command), BYOK
  endpoints incl. test-connection.
- Create the Python ee/ce seam: `pi_dash/ee/assistant/model_provider.py` OSS stub
  (04-cloud.md §3 — does not exist today).
- Create the `pi_dash/core/` package per the extract-vs-copy decision table (02 §5): extract
  `check_project_role` from the `@allow_permission` decorator into `core/permissions.py`, move
  workspace role helpers there from `runner/services/permissions.py` (re-export shim in place),
  add `core/querysets.py::user_issues_queryset`. Search needs nothing — tools import the
  already-extracted `pi_dash/search/issue.py::issue_search_queryset`.
- Runtime: agent definition, deps, instructions template, model resolution, history load
  (from `AssistantTurn.model_messages`).
- Read-only tools (`list_projects`, `list_states/labels/members`, `search_issues`, `get_issue`,
  `list_my_issues`, `get_run_status`) + `_scoping.py` parity layer.
- Celery turn task (`run_stream_events` loop, event mapping per 02 §8.2), sweep task + beat
  registration, SSE events endpoint. End-to-end testable via curl.

**Phase 2 — Write tools + frontend**

- **Step 0 (own PR, refactor-only): extract the shared chat kit** from the runner chat page and
  refactor that page to consume it — see 06-chat-ui-refactor.md. Lands before any assistant UI;
  verified behavior-preserving against existing runner chat.
- Write tools (`create_issue`, `update_issue`, `create_comment`) with attribution
  (markdown → `description_html` per 02 §4.2; comment serializer exposes
  `speaker_type`/`speaker_label`).
- `dispatch_coding_run` via `handle_issue_state_transition`.
- Web (built on the kit): settings tab (types + constants packages), home widget, thread route,
  streaming chat, tool activity rows, attribution badges.

**Phase 3 — Cloud enablement (private repo) — minimal after the BYOK-only decision**

- SSM `ASSISTANT_ENCRYPTION_KEY`, `ASSISTANT_BLOCK_PRIVATE_URLS=True`, (optionally) tighter
  message-throttle env, observability dashboards. **No quota / plan / billing work in MVP**
  (deferred design parked in 04-cloud.md §2).

(Phases 1+2 ship together as the feature; the split is build order, not release gates.
Phase 3 is now a config-and-dashboards-sized private-repo PR.)

## Testing strategy

- **Access-control parity tests (the load-bearing suite):** for each tool, a matrix test —
  workspace admin / member / guest / non-member / member-of-other-workspace — asserting the tool's
  visible set and write outcomes **equal the corresponding API endpoint's** for the same fixture
  data. Implementation: call tool helper and view (DRF test client) side by side, compare ids.
  Any divergence fails CI. This directly encodes requirement 5.
- **Runtime tests with `TestModel`/`FunctionModel`** (pydantic-ai's test doubles — no network):
  loop behavior, history persistence round-trip, usage capture, max-iteration cap, cancellation.
- **Concurrency test:** two simulated users, interleaved turns on one runtime, assert no
  cross-thread/cross-tenant bleed in history rows or tool scoping.
- **SSE contract test:** replay + live + resume-after cursor (event seq), delta-event pruning
  after turn completion, cancel-mid-stream emits `turn_cancelled` and finalizes the row.
- **Endpoint semantics tests:** 409 `turn_active` on concurrent post, 409 `thread_full` at cap,
  422 `llm_config_missing`, title generation on first message, retry-creates-new-turn.
- **Crypto tests:** never-plaintext at rest, key absent from logs/serializers/errors, MultiFernet
  rotation (old-key rows still decrypt; re-encrypt command migrates them).
- **Throttle test:** message POST throttle enforces the configured rate; guests receive 403 on
  every assistant endpoint. (Quota tests move to the post-MVP quota work parked in 04 §2.)
- E2E (Playwright, per repo testing rules): configure BYOK against a mocked OpenAI-compatible
  server fixture → send message on landing page → see streamed reply → issue created with badge.

## Key risks & mitigations

| Risk                                                  | Mitigation                                                                                                                                          |
| ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| Scoping drift between views and tools over time       | Shared helpers where possible; parity test matrix in CI for every tool (fails on divergence)                                                        |
| Prompt injection via issue/comment content            | Tools capped at the user's own permissions; no delete tools v1; quoted content delimited; full tool-call audit trail in transcript                  |
| Weak OSS models flail at tool-calling                 | Honest provider errors in-thread; known-good model suggestions in settings; (later) prompted-JSON fallback tier                                     |
| BYOK key leakage                                      | Fernet at rest, write-only API, decrypt only in worker, scrubbed errors, no logging                                                                 |
| SSRF via base_url (cloud)                             | Cloud-on blocklist for private/link-local ranges                                                                                                    |
| Long turns vs worker recycling                        | Celery execution, SSE resume by seq, hard turn timeout + iteration cap                                                                              |
| Unbounded history growth                              | v1 thread cap (200 messages → `thread_full`) + new-thread nudge; Phase-2 compaction                                                                 |
| pydantic-ai upgrades break persisted `model_messages` | exact pin (≥ PR #4421 release); round-trip smoke test of stored rows gates version bumps                                                            |
| Celery redelivery re-executing write tools            | `max_retries=0`, no `acks_late`; sweep fails crashed turns; retry = new turn                                                                        |
| Auto-execute writes surprise users                    | Tool-activity rows make every action visible + linkified; objects attributed "via assistant"; future per-user confirm toggle if feedback demands it |

## Open questions — ALL RESOLVED (none remain)

Resolved during design review:

- **Issue description format** → (02 §4.2): markdown → sanitized `description_html` via the
  `bgtasks/github_sync_task.py:68-95` pattern; `description_json = {}` (no Python Tiptap
  converter exists; building one is out of scope).

Resolved by product owner, 2026-06-11:

1. **Home widget thread behavior** → **new thread per submission**; recent threads listed for
   one-click continuation (03 §1).
2. **Quotas** → **scoped out of MVP entirely; BYOK-only**. No plan field, no billing/entitlement
   work; rate throttle is the only brake. Deferred design parked in 04 §2.
3. **Guest role UX** → **hidden from guests**: UI renders the widget/routes for role ≥ MEMBER
   only; backend 403 `role_not_allowed` on all assistant endpoints (02 §9.1, 03 §1).
4. **Thread deletion** → **hard delete** (active turn cancelled first); soft-delete retention
   deferred until an audit-log plan needs it.

Resolved by code/registry inspection, 2026-06-11: 5. **pydantic-ai pin** → `pydantic-ai-slim[openai,anthropic]==1.107.0` (2.0 betas excluded;
upgrades gated by the `model_messages` round-trip smoke test) — 02 §0. 6. **Parity layer extract-vs-copy** → per-site decision table in 02 §5 (`pi_dash/core/` package;
search already extracted; issue/comment querysets COPY + equivalence tests). 7. **Base instructions** → full v1 text in 02 §2.1; structural elements are fixed contract,
wording tunable under runtime tests.

## Out of scope for v1 (explicitly)

- Plan quotas and billing gating (BYOK-only MVP — deferred design parked in 04 §2)
- Confirm-before-write UX (auto-execute decided; revisit on feedback)
- Platform-provided keys for paid users (Phase: future, seam designed in 04-cloud.md §3)
- Cross-workspace assistant, mobile, slash-commands in issue composer
- Prompted-JSON fallback for non-tool-calling models
- Delete/destructive tools, bulk operations
- Compaction/summarization of long threads (cap only in v1)
