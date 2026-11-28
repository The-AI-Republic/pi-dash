# Design: model selection for the assistant and the Pi Dash Agent

- **Status:** revision 2 (2026-11-22), implementation-ready draft. Decisions D-1 to D-12 (section 3) were made by the product owner on 2026-11-21/22. Revision 2 is the design review against the code; its changes are listed in section 13.
- **Code baseline:** pi-dash `origin/main` at `ed077eaf5`; private-pi-dash `origin/main` at `e6e83f2`. Paths are relative to each repo. Read from the code; nothing was run.
- **Scope:** server changes target the Rust API only (`rust-api/` in OSS, `rust-cloud/` in private). Django code is out of scope; Django migration files are touched only if the schema-ownership work has not landed (section 9.1).
- **Terms:**
  - **Surface:** where a model is used: `web_assistant` (assistant chat in the browser), `desktop_assistant` (assistant chat in the desktop app) and `agent` (the Pi Dash Agent, the built-in engine run by the desktop daemon, for both its chats and the issue runs it executes).
  - **Catalog:** the god-mode "OpenHub (assistant models)" section of the Runner Models page (`rust_runner_model_catalog` rows with `agent = openhub`).
  - **Connection:** a saved provider in cloud (`cloud_assistant_user_llm_connection`): the OpenHub row, BYOK rows, the Codex row.
  - **Choice:** one entry a user can pick: a model ID plus the connection that serves it.

## 1. Summary

- God mode edits **one** model list (the catalog). The web assistant, the desktop assistant and the Pi Dash Agent all offer it.
- Each user has **one saved choice per surface**, stored in the existing JSONB settings bag on `profiles` (`profiles.settings`, namespace `assistant_models`). Changing the model on one surface does not change the others.
- A BYOK key can carry **several model IDs**, typed by the user and sent to the provider as typed. A key must have at least one model; removing the last model removes the key.
- Every model ID is **smoke-tested** when it is added.
- The assistant picker shows **one merged list**: catalog models first, then BYOK models grouped by key, e.g. `modelB`, `modelB (BYOK · openrouter.ai)`.
- A saved choice that is no longer valid **falls back to the catalog default** (today `openhub_free_llm`). The saved entry is kept and the UI says what happened.

## 2. Today

### 2.1 The model list

- `model_options_with_catalog` (`private-pi-dash/rust-cloud/crates/cloud-api/src/openhub/llm.rs:132`): enabled catalog rows in `sort_order`; if there are none, `OPENHUB_ASSISTANT_MODELS`, then the hardcoded three (`crates/cloud-db/src/config.rs:422`).
- `default_model_with_catalog` (`llm.rs:145`): the preselected catalog row, else `OPENHUB_DEFAULT_MODEL` (default `openhub_free_llm`), else the first option.
- The list reaches the browser as `openhub_model_options` (IDs only) in the connections list response (`crates/cloud-api/src/assistant/connections_routes.rs:498`).
- **The catalog `label` is not shown.** The picker calls `modelChoices(models, {}, OPENHUB_MODEL_DESCRIPTIONS, current)` (`apps/web_new/src/features/cloud-assistant/lib/rows.ts:258`, `assistant/components/OpenHubModelPicker.tsx:73`) with an empty labels map, so names come from the hardcoded `OPENHUB_MODEL_DESCRIPTIONS` (`rows.ts:48`). Codex already has the pattern to copy: `codex_model_labels` next to `codex_model_options`.

### 2.2 The selection

- `cloud_assistant_user_llm_connection` (`private-pi-dash/pi_dash_cloud/assistant/models.py`): one OpenHub row and at most one Codex row per user (unique constraints), any number of BYOK rows, each with **one** `model_name`. Exactly one row has `is_selected`.
- `select_in` (`crates/cloud-api/src/assistant/connections.rs:721`) selects a row, deselects the others, and mirrors the choice into single-slot stores: BYOK is copied into the OSS `assistant_user_llm_config`; Codex marks the codex credential active; OpenHub clears both.
- The composer's `OpenHubModelPicker` writes the OpenHub row's model through the PATCH connection-detail endpoint (`update_model`, `connections.rs:946`).
- Everything resolves from that one selection:
  - turns and titles: `llm_selection` (`provider.rs:324`), `resolve_model_for_user` (`:736`), `generate_title_for_user` (`:813`), `model_label_for_user` (`:904`), `CloudTurnSeam::resolve_model` (`:1856`) and `model_label` (`:1994`);
  - the desktop agent: `agent_model_profile_for_user` (`provider.rs:1216`) → `openhub_model_for` (`connections.rs:1169`).
- **Consequence:** one choice drives all three surfaces. A user who selects BYOK for web chat also breaks the desktop agent (`byok_not_supported_on_desktop`).

### 2.3 Other callers of the same gates

The "is an LLM configured" gate and the agent profile are also read outside assistant chat. These decide whether an issue run can go to the Pi Dash Agent, so they are `agent`-surface questions:

- `rust-api/crates/api/src/v1_work_items/handlers_actions.rs:997-1070`: `has_usable_llm_config` verdicts for the run-creator universe through the seam, and an `llm_profile` built from the **CE function directly** (`pidash_services::assistant::seams::pidash_agent_profile_for_user(key)`, `:1032`), not through the seam;
- `rust-api/crates/api/src/app_project/handlers_project.rs`, `issue_transition.rs`, `turn/tools_local_store.rs`: the same gate and profile;
- `rust-api/crates/api/src/pidash_agent/endpoints.rs`: the daemon's engine profile and model-token endpoints;
- `rust-api/crates/api/src/assistant/messages.rs` (`llm_gate_allows`, `:438`): the gate before a chat message is accepted.

### 2.4 What a turn knows

- `post_message` (`rust-api/crates/api/src/assistant/messages.rs:192`) inserts the `assistant_turn` row (`:300`) with no surface. `TurnContext` (`rust-api/crates/jobs/src/assistant/run_turn.rs:484`) has no surface field; `enqueue_run_turn` passes only the turn ID.
- The server already tells a desktop session at request time: `is_desktop_from_extension` (`agent_profile.rs:159`).
- Desktop assistant chat runs server-side like web: the desktop build relays the same SSE frames over its native stream (`apps/web_new/src/features/assistant/api/events.ts`).

### 2.5 BYOK writes and the connection test

- **OSS:** `GET/PUT/DELETE /api/users/me/ai-assistant/config/` and `POST .../config/test/` (`rust-api/crates/api/src/assistant/llm_config.rs:76-78`) on the one-row-per-user `assistant_user_llm_config`.
- **Cloud has two write paths:** the connections API (`connections_routes.rs:121-134`: list/create, detail PATCH/DELETE, `select`, `disconnect`) and a cloud override of the same `/config/` endpoints (`crates/cloud-api/src/assistant/config.rs`, "credential-mode" config; its test endpoint is throttled to **6/minute**).
- **The probe** (`rust-api/crates/services/src/assistant/probe.rs`, used by both builds): one chat request `"Reply with the single word: ok"` with `max_tokens: 16`, a 60 s timeout, no redirects. `classify_probe` (`:43`) maps the result by keywords to three codes: `provider_auth_failed` (401 / "api key"…), `model_invalid` ("model" / "not found"…), `provider_unreachable` (timeouts, connection errors, and everything else, including 429 and 5xx).

### 2.6 Where user preferences live

- `profiles.settings` (JSONB, `rust-api/crates/db/src/entity/profiles.rs:47`) is a namespaced bag `{namespace: {key: value}}` (`rust-api/crates/services/src/user_settings.rs`).
- The profile PATCH validates a client patch against a declared schema and **rejects unknown namespaces** (`validate_settings_patch`, `:169`); `merge_settings` (`:220`) keeps every stored namespace whose value is an object, declared or not. OSS declares no namespaces; the cloud adds its own (`rust-cloud/crates/cloud-api/src/assistant/user_settings.rs`).

### 2.7 Out of scope: editor AI

The page/editor AI endpoints (`/api/workspaces/{slug}/ai-assistant/`, `/rephrase-grammar/`; `rust-api/crates/api/src/app_integrations/handlers_external.rs`) use the instance-level LLM config in `instance_configurations`, not a user's choice. This design does not change them.

## 3. Decisions

| ID | Decision |
|---|---|
| D-1 | One catalog, edited in god mode, feeds all three surfaces. |
| D-2 | Each surface has its own saved choice. The list is shared; the choice is not. |
| D-3 | The choice is stored on `profiles` as JSONB (revision 2: in the existing `profiles.settings` bag, section 4.1). |
| D-4 | Changing the model in the composer picker changes that surface's default. No per-conversation model. |
| D-5 | All three choices are made in Assistant Settings. The desktop agent's status area links there. |
| D-6 | BYOK model IDs are typed by the user. No provider model discovery. |
| D-7 | A BYOK key holds a list of model IDs. A new key cannot be saved without at least one. |
| D-8 | Removing a model is always allowed. Removing the last one asks for confirmation and then deletes the key. |
| D-9 | Every added model ID is smoke-tested before it is saved. |
| D-10 | The assistant picker shows catalog and BYOK models in one list. The same model ID through two connections is two choices. |
| D-11 | An invalid saved choice falls back to the catalog default. |
| D-12 | The Pi Dash Agent stays OpenHub-only (BYOK keys are never sent to a client). OSS gets the same multi-model BYOK list and per-surface choice. Temporary smoke-test failures may be saved anyway. |

## 4. Data model

### 4.1 Per-surface choice: `profiles.settings.assistant_models` (OSS, no migration)

Stored in the existing bag under the namespace `assistant_models`:

```json
{
  "assistant_models": {
    "web_assistant":     { "model": "moonshotai/kimi-k3", "connection_id": "6f1c…" },
    "desktop_assistant": { "model": "gpt-6", "connection_id": "a93e…" },
    "agent":             { "model": "deepseek/deepseek-v4-pro" }
  }
}
```

- **Not declared in the settings schema, on purpose.** The generic profile PATCH then refuses the namespace (`unknown settings namespace`), so the only writer is the endpoint in section 6.1, which validates the connection and the model. `merge_settings` keeps the namespace when other namespaces are patched, because its value is an object.
- Keys: `web_assistant`, `desktop_assistant`, `agent`. `model` is required in an entry. `connection_id` is optional:
  - cloud: the connection that serves the model. Missing means the user's OpenHub connection;
  - OSS: always absent.
- `agent` entries never carry a BYOK or Codex `connection_id` (D-12).
- No foreign key. A `connection_id` that no longer exists is an invalid choice and falls back (section 5).
- **Atomic writes:** the endpoint writes one surface with a single `UPDATE profiles SET settings = jsonb_set(coalesce(settings,'{}'), '{assistant_models,<surface>}', $entry, true)` (and `#-` for clear), not a read-modify-write of the whole bag. The profile PATCH still rewrites the whole bag (`merge_settings`); a PATCH racing a model-selection write can restore the previous entry. This is accepted: both are rare user actions, and the user sees the result in the picker.
- The value is returned in `GET /api/users/me/profile/` as part of `settings`; the frontend reads choices from section 6.1, not from there.

### 4.2 Surface type (OSS)

- New `Surface` enum in `pidash-types` (`web_assistant | desktop_assistant | agent`) with `as_str` / `parse`, shared by the API, jobs, services and the cloud crate through `pidash-sdk`.

### 4.3 `assistant_turn.surface` (OSS, migration)

- New column `surface varchar(32) NOT NULL DEFAULT 'web_assistant'`.
- `post_message` sets it to `desktop_assistant` when `is_desktop_from_extension` is true for the request, else `web_assistant`. The client never sends it.
- `TurnContext` gains `surface: Surface`, read with the turn in `load_context`.
- Per turn, not per thread: a thread can be continued from either surface.

### 4.4 BYOK model lists (migrations)

**Cloud** (`cloud_assistant_user_llm_connection`): new columns

- `models text[] NOT NULL DEFAULT '{}'` (BYOK rows; OpenHub and Codex rows leave it empty);
- `model_verified_at jsonb NOT NULL DEFAULT '{}'` (model ID → ISO timestamp of the last passing test; no entry = "not verified").

**OSS** (`assistant_user_llm_config`): the same two columns.

**Backfill (both):** `models = ARRAY[model_name]` where `model_name <> ''`; `model_verified_at = jsonb_build_object(model_name, last_verified_at)` where `last_verified_at` is set.

**Validation on every write (both):** trim; drop empty entries and duplicates within one key; each ID at most 255 characters; at most **20** IDs per key (revision 2: lowered from 50 to bound the smoke test, section 7.2); a new key needs at least one ID; an update to zero IDs is rejected (`models_required`).

### 4.5 Columns that become legacy

- Connection `is_selected` / `model_name`, and `assistant_user_llm_config.model_name` / `last_verified_at`, stay as the fallback in section 5 step 2.
- Writers keep `model_name = models[0]` and `last_verified_at = model_verified_at[models[0]]` so older readers keep working.
- Removal is a follow-up (section 11).

## 5. Resolving a surface's model

One resolver per build: `(user, surface) → Resolution { effective: Choice | None, fallback_reason }`. Every caller in sections 2.2 and 2.3 goes through it. Order:

1. **Saved entry** `settings.assistant_models[surface]`, if valid:
   - cloud, OpenHub (or no `connection_id`): the OpenHub lane is usable for the user (`llm_enabled` and gateway scopes), the model is in the enabled catalog list, and the free-plan rule allows it (as `openhub_model_for` does today: a free-plan user's non-default model resolves to the default);
   - cloud, BYOK: the connection exists, **belongs to the user**, has a key, and lists the model. Never for `agent`;
   - OSS: the config has a key and lists the model.
2. **Legacy selection**, checked the same way:
   - cloud: the `is_selected` connection and its `model_name`. A selected **Codex** row is not valid for any surface (the chat engine cannot run it, `MSG_CODEX_CHAT_UNSUPPORTED`); a selected BYOK row is not valid for `agent`. Both go on to step 3;
   - OSS: `models[0]` of the config.
3. **Catalog default** (`default_model_with_catalog`) on the OpenHub connection, when the lane is usable. Cloud only.
4. **First BYOK choice:** the first model of the user's oldest BYOK connection with a key. Not for `agent`.
5. **None:** the existing failures: `llm_config_missing` for chat; the existing `PidashAgentProfile` reason codes (`gateway_scopes_missing`, `llm_config_missing`, …) for `agent`.

`fallback_reason` is set whenever step 1 did not apply: `none_saved`, `model_disabled`, `model_removed`, `connection_removed`, `plan_blocked`, `lane_unavailable`, `not_allowed_for_surface`. The saved entry is **never rewritten** by a fallback.

### 5.1 Seam changes (OSS trait, cloud implementation)

`AssistantModelSeam` (`rust-api/crates/services/src/extensions.rs:173`) and `TurnSeam` (`rust-api/crates/jobs/src/assistant/run_turn.rs`):

| Method | Change |
|---|---|
| `has_usable_llm_config(user_id, has_api_key)` | gains `surface`. True when the resolver finds a choice. |
| `pidash_agent_profile(…)` / `pidash_agent_credential(…)` | resolve with `Surface::Agent`. |
| `model_label(…)` | gains `surface`. |
| `generate_title(TitleRequest)` | `TitleRequest` gains `surface`. |
| new `resolve_choice(user_id, surface)` | returns the `Resolution` (used by section 6.1 GET). |
| new `validate_choice(user_id, surface, entry)` | section 6.1 PUT validation. |
| `TurnSeam::resolve_model` / `model_label` | read `ctx.surface`. |

All callers in section 2.3 pass `Surface::Agent`. The direct CE call at `v1_work_items/handlers_actions.rs:1032` moves behind the seam, so the cloud build answers it. **Check during implementation** whether that direct call is already wrong in cloud today (it ignores the OpenHub lane); if so, fix it in the same change.

### 5.2 Cloud specifics

- BYOK turns read the provider, base URL and key **from the connection row named by the choice**, not from the OSS single-slot copy. `select_in` keeps writing that copy for older readers. Implementation lists every cloud reader of `assistant_user_llm_config` (`provider.rs` uses `get_config` in `has_usable_llm_config` and the agent profile, `config.rs` serializes it) and moves each to the resolver.
- A user whose web choice is BYOK no longer gets `byok_not_supported_on_desktop` for the agent: the agent resolves to OpenHub on its own (steps 1 to 3).
- `fetch_openhub_context_window` is unchanged; it runs on the resolved agent model.

### 5.3 OSS specifics

`CeAssistantSeam` (`rust-api/crates/api/src/assistant/model_seam.rs`) and `resolve_model_for_user` (`rust-api/crates/services/src/assistant/seams.rs:60`) use steps 1, 2 and 5. The OSS agent profile stays unavailable for every user.

## 6. API

### 6.1 Per-surface choice (OSS route; validation through the seam)

`GET /api/users/me/ai-assistant/model-selection/`

```json
{
  "surfaces": {
    "web_assistant": {
      "saved":     { "model": "modelC", "connection_id": "a93e…" },
      "effective": { "model": "openhub_free_llm", "connection_id": "6f1c…" },
      "fallback_reason": "model_removed"
    },
    "desktop_assistant": { "saved": null, "effective": { "…": "…" }, "fallback_reason": "none_saved" },
    "agent":             { "saved": null, "effective": null, "fallback_reason": "lane_unavailable" }
  }
}
```

`PUT /api/users/me/ai-assistant/model-selection/{surface}/`, body `{ "model": "…", "connection_id": "…" }`:

- `400` with `unknown_surface`, `model_not_available`, `connection_not_found` (also for another user's connection; never reveal that it exists), `not_allowed_for_surface` (BYOK or Codex for `agent`), `plan_blocked`;
- `200` with the same per-surface object as GET;
- writes one surface atomically (section 4.1).

`DELETE /api/users/me/ai-assistant/model-selection/{surface}/` clears the entry (use the default).

Any session may set any surface: Settings edits all three from either host (D-5). The composer picker sets `desktop_assistant` in the desktop build and `web_assistant` in the browser. Which surface a **turn** ran on is decided by the server (section 4.3), never by this endpoint.

Session auth only, same permission wrapper as the other `/api/users/me/ai-assistant/` routes. Throttle: the existing per-user assistant config throttle class.

### 6.2 Model list with labels (cloud)

- The connections list response gains `openhub_model_labels` (model ID → catalog `label`, only non-empty labels), next to `openhub_model_options`.
- Each serialized BYOK connection gains `models` and `model_verified_at`.

### 6.3 BYOK writes

All three write paths: OSS `/config/`, cloud `/config/` override, cloud `/connections/`.

- Create / PUT accept `models: string[]` (required, at least one). A body with only `model_name` is treated as `models: [model_name]` for older clients.
- PATCH (cloud connection detail) and PUT (`/config/`) accept a full `models` list; the server diffs it against the stored list to find **new** IDs. An empty list is `400 models_required`.
- Every write runs the smoke test (section 7.2) and returns per-ID results:

```json
{ "models": [
    { "model": "gpt-6",   "status": "verified" },
    { "model": "gpt-6.3", "status": "failed", "code": "model_invalid", "detail": "Model not accepted by the provider." },
    { "model": "gpt-6.2", "status": "unverified", "code": "provider_unreachable" }
] }
```

- A write is rejected (`400 model_test_failed`, same per-ID body, nothing saved) when any new ID is `failed`, or when an ID is `unverified` and the body did not set `"save_unverified": true` ("Save anyway").

### 6.4 Per-model test

`POST …/config/test/` (OSS and cloud override) and a new `POST /api/users/me/ai-assistant/connections/{id}/test/` (cloud) accept `{ "model": "…" }` to test one stored ID, update `model_verified_at`, and return one result object. Existing throttle (6/minute) applies.

## 7. Behaviour

### 7.1 The merged list

For `web_assistant` and `desktop_assistant`:

1. catalog models, in catalog order, labelled with the catalog label, else the built-in description, else the ID; with the existing free-plan credit badges;
2. then each BYOK connection in creation order, each model ID labelled `<id> (BYOK · <base-URL host>)`, or `<id> (BYOK · <provider>)` when there is no base URL. Always selectable; no credit badge.

Codex models are not in the assistant lists. For `agent`: catalog models only.

The picker marks the effective choice. When `fallback_reason` is set and `saved` is not null, the picker and Settings show one line, e.g. "modelC is no longer available. Using OpenHub free model."

### 7.2 Smoke test

- **When:** a new key (all its IDs); a new ID on an existing key (that ID only); the key or base URL of an existing key changed (all its IDs).
- **What:** the existing probe (`probe.rs`), unchanged prompt and `max_tokens: 16`, after the existing base-URL SSRF check. At most **4 probes in parallel** per request and a **20 s** per-probe timeout for save-time tests (the 60 s timeout stays for the single-model test button), so a 20-model save finishes in about 100 s at worst. The frontend shows per-ID progress.
- **Result mapping** (on the existing `classify_probe` codes):

| Probe code | Status | Save |
|---|---|---|
| success | `verified` | saved, `model_verified_at[id] = now` |
| `provider_auth_failed`, `model_invalid` | `failed` | blocked; the form shows the detail next to the ID |
| `provider_unreachable` | `unverified` | "Save anyway"; saved without a `model_verified_at` entry |

- `provider_unreachable` also covers a wrong base URL, so "Save anyway" can save a dead endpoint. That is accepted: the model shows "not verified" and the first turn shows the provider error with the model and connection named.
- A new key is saved only if no new ID is `failed` (the user fixes or removes those first).

### 7.3 Removing models and keys

- Removing an ID that is not the last saves the shorter list.
- Removing the last ID asks: "Removing gpt-6 also removes this OpenAI key. Remove both?" On confirm the UI calls the existing connection DELETE (cloud) or config DELETE (OSS). The server never deletes a key because of an empty list.
- Saved choices that pointed at the removed ID or key fall back (section 5).

### 7.4 Assistant Settings (D-5)

- A "Models" section with three pickers (Web assistant, Desktop assistant, Pi Dash Agent), each showing its effective choice and fallback notice. OSS shows Web and Desktop only (no agent).
- Files: OSS `apps/web_new/src/features/profile-assistant/` (`AssistantSettingsPage.tsx`, `AssistantSections.tsx`) and the cloud edition's `apps/web_new/src/features/cloud-assistant/` (`CloudAssistantTab.tsx`, `lib/rows.ts`).
- The BYOK editor lists each key's IDs with add/remove, a per-ID status (verified / not verified / failed) and a per-ID Test button. The per-connection "Use this" (`select`) control is removed from the new UI.
- The desktop agent status area (`apps/web_new/src/features/runners/runtime/service.ts` and its UI) shows the agent model from the engine status and links to the Settings section. A changed agent model takes effect at the engine's next profile fetch; the daemon already restarts the engine once idle when the profile changes (`runner/src/pidash_agent/manager.rs:433`). No daemon change.

### 7.5 Composer picker (D-4)

- `OpenHubModelPicker` (and `assistant/lib/openhubPicker.ts`) becomes the merged picker and writes `PUT model-selection/{surface}`. It no longer PATCHes the OpenHub connection.
- It renders whenever the surface has at least one choice, not only on the OpenHub lane.

### 7.6 Old clients

- Older desktop builds keep calling the connection PATCH / `select`. Those still write the legacy fields, which only affect surfaces with no saved entry (step 2). Nothing breaks; their picker changes only "the default for surfaces you have not set".
- The `select` and PATCH-model endpoints stay as they are until the legacy fields are removed (section 11).

## 8. OSS and cloud split

| Piece | OSS (`pi-dash`) | Cloud (`private-pi-dash`) |
|---|---|---|
| `Surface` type, seam signature changes | ✓ | implement the new signatures |
| `assistant_turn.surface`, `TurnContext.surface`, stamping in `post_message` | ✓ | |
| `settings.assistant_models` read/write helpers | ✓ | uses them through `pidash-sdk` |
| `model-selection` endpoints | ✓ route + OSS validation | `validate_choice` / `resolve_choice` |
| Resolver | OSS (config only) | `provider.rs` / `connections.rs` |
| Agent gates in section 2.3 pass `Surface::Agent` | ✓ | follows from the seam |
| BYOK `models` / `model_verified_at` | `assistant_user_llm_config` + `/config/` | connection row + `/connections/` + `/config/` override |
| Smoke test on save, per-model test | `llm_config.rs`, `probe.rs` (parallel cap, save timeout) | `connections_routes.rs`, `config.rs` |
| `openhub_model_labels` | | connections list |
| Merged picker, Settings "Models", BYOK editor | `web_new` (both editions) | |

## 9. Implementation plan

### 9.1 Migrations

Schema ownership is moving from Django to a native Rust migrator (NEWPIDASH-379 design, PR #3463; migrator NEWPIDASH-380, PR #3468).

- If NEWPIDASH-380 has merged when this starts: add the OSS columns (sections 4.3, 4.4) as Rust migrations, and the cloud columns through the cloud's migration path that 380 defines.
- If not: add Django migrations (`apps/api/pi_dash/assistant/migrations/0005_…`; `pi_dash_cloud/assistant/migrations/…`) plus the matching cloud migration port (`cloud-migration` PORTS), as the schema-ownership doc describes for the transition window.
- `profiles` needs no migration (section 4.1).

### 9.2 Order

The cloud crate depends on OSS through the `pidash-sdk` pin, so OSS lands first.

1. **OSS-1: surface + resolver + selection API.** `Surface`, `assistant_turn.surface` + stamping, `TurnContext.surface`, seam signature changes with CE defaults, the OSS resolver, the agent-gate callers passing `Surface::Agent`, `model-selection` endpoints, `settings.assistant_models` helpers. No behaviour change for anyone (every bag is empty).
2. **OSS-2: BYOK multi-model + smoke tests (OSS config).** Columns + backfill on `assistant_user_llm_config`, `/config/` accepting `models`, save-time tests, per-model test.
3. **OSS-3: web_new UI.** Merged picker, Settings "Models", BYOK editor, last-model confirmation, fallback notice, catalog labels when present.
4. **CLOUD-1: pin bump + routing.** Bump the OSS pin; implement the new seam methods and the cloud resolver; `CloudTurnSeam` reads `ctx.surface`; agent profile via the resolver; BYOK turns read the chosen connection row.
5. **CLOUD-2: connections.** `models` / `model_verified_at` columns + backfill; `/connections/` and the `/config/` override accept `models` and run save-time tests; `connections/{id}/test/`; `openhub_model_labels`.

Each slice ships alone: after 1 to 3 the OSS build is complete; after 4 and 5 the cloud is.

## 10. Tests

- Resolver (both builds): one test per step and per `fallback_reason`, including Codex legacy selection, BYOK legacy selection on `agent`, the free-plan rule, a deleted connection, another user's `connection_id`.
- `post_message` stamps `desktop_assistant` only for desktop sessions; a client cannot set it.
- `model-selection`: every `400` in section 6.1; PUT writes only its own surface (other namespaces and surfaces untouched); generic profile PATCH with `assistant_models` is rejected as an unknown namespace; a profile PATCH of another namespace keeps `assistant_models`.
- Agent gates (section 2.3) answer from the `agent` surface: a user whose web choice is BYOK can still have issue runs go to the Pi Dash Agent.
- BYOK validation: trim, duplicates, 255 characters, 20-ID cap, empty list rejected, `model_name`-only bodies still work; backfill.
- Smoke test: each probe code maps to `verified` / `failed` / `unverified`; "Save anyway" required for `unverified`; key or base-URL change re-tests every ID; parallel cap respected.
- Frontend: merged order and labels, fallback notice, last-model confirmation calls DELETE, composer writes only its own surface.
- Contract tests under `rust-api/contract-tests/assistant/` and `rust-cloud/contract-tests/assistant/` for new and changed endpoints.

## 11. Follow-ups (not in this design)

- Remove `is_selected` / `model_name`, the `select` endpoint and the `select_in` copy into `assistant_user_llm_config` once no reader or supported client depends on them.
- Per-conversation model override (a `model` on the thread); the per-surface choice becomes the thread's default.
- BYOK for the Pi Dash Agent through a server-side proxy.
- Per-surface availability on catalog rows (e.g. hide a model from the agent).
- Make the profile PATCH write per namespace atomically, removing the race noted in section 4.1.

## 12. Open items

None. Resolved in revision 2:

- **Free plan and BYOK** (product owner, 2026-11-22): BYOK is allowed on every cloud plan. Checked in code: the quota middleware (`rust-cloud/crates/cloud-api/src/quotas.rs`) limits only project and work-item creation paths (`CREATE_LIMITED_PATHS`, `:88`), and the connection and `/config/` handlers (`assistant/connections_routes.rs`, `assistant/config.rs`) have no plan check. The only plan rule on models stays the free-plan rule on paid OpenHub models (section 5 step 1).

## 13. Revision 2: review changes

1. **Storage:** the choice moves from a new `profiles.assistant_models` column to the existing `profiles.settings` bag (namespace `assistant_models`). No migration; the namespace is left out of the settings schema so only the validated endpoint can write it; writes use `jsonb_set`.
2. **Missed callers:** run-creation and issue-transition gates, the daemon engine endpoints and `tools_local_store` also read the gate and agent profile (section 2.3). They now resolve with `Surface::Agent`; one of them calls the CE function directly and moves behind the seam.
3. **Seam contract:** the exact trait changes are listed (section 5.1), plus a shared `Surface` type.
4. **Cloud BYOK has two write paths** (`/connections/` and the `/config/` override); both are covered.
5. **Smoke test:** reuse the existing probe and its three codes instead of a new prompt; status mapping defined on those codes; parallel cap, save-time timeout, 20-ID cap; the 6/minute throttle stays on the manual test only.
6. **Legacy rules made explicit:** Codex and BYOK legacy selections and the agent; old desktop builds; the `select` control leaves the new UI.
7. **Implementation plan:** migration path tied to the schema-ownership work, OSS-before-cloud order through the SDK pin, five shippable slices.
8. **Scope note:** editor AI endpoints use the instance config and are out of scope. Agent model changes apply at the engine's next idle restart.
