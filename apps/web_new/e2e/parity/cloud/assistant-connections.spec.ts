// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): AI Assistant connections tab, row
// CLOUD-032 (the tab manages provider connections), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud profile area lands there.
//
// Observed behavior notes (cloud oracle): each saved connection renders
// a card with its kind-appropriate title, an in-use marker on the
// selected one, a model control — a fixed picker for managed kinds, free
// text with suggestions for self-supplied keys — and a verified
// timestamp once a test succeeds. Switching, testing, removing
// self-supplied rows, and disconnecting the account row all confirm or
// report errors inline. The account row is permanent: unusable until an
// account backs it, disconnected rather than removed. An empty list
// points at adding the first connection. The empty panel never occurs
// live — the account row always seeds where provider sign-in is
// offered — so it runs against a stubbed list read; connected account
// states run through a planted credential, since real provider OAuth is
// unreachable from runners. A successful test needs a live provider, so
// only the failure report is proven; the verified stamp is planted.
import { test, expect } from "../fixtures";
import {
  cloudAssistantConnectionsRaw,
  cloudAssistantCreateByokRaw,
  cloudAssistantSelectRaw,
  cloudAssistantTestRaw,
  cloudBrowserCookies,
  cloudPlantCodexCredential,
  cloudStampConnectionVerified,
  uniqueEmail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { cloudOnboardedUser } from "./cloud-user-setup";

const CONNECTIONS_PATH = "**/api/users/me/ai-assistant/connections/";

type ConnectionRow = {
  id: string;
  kind: string;
  model_name: string;
  is_selected: boolean;
  connected?: boolean;
};

function rowsOf(payload: Record<string, unknown>): ConnectionRow[] {
  return (payload["connections"] as ConnectionRow[]) ?? [];
}

async function createByok(
  cookie: string,
  opts: { baseUrl?: string; model?: string; key?: string } = {}
): Promise<ConnectionRow> {
  const created = await cloudAssistantCreateByokRaw(cookie, {
    provider_kind: "openai_compatible",
    base_url: opts.baseUrl ?? "https://openrouter.ai/api/v1",
    model_name: opts.model ?? "meta-llama/llama-3.3-70b-instruct",
    api_key: opts.key ?? "parity-440-conn-key",
  });
  expect(created.status).toBe(201);
  return created.payload as unknown as ConnectionRow;
}

test(
  specTitle(["CLOUD-032"], "empty list points at adding the first connection"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Empty");
    await driver.page.route(CONNECTIONS_PATH, async (route) => {
      const request = route.request();
      if (request.method() !== "GET" || new URL(request.url()).pathname !== "/api/users/me/ai-assistant/connections/") {
        await route.fallback();
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          connections: [],
          codex_model_options: ["gpt-5.6-sol"],
          codex_model_labels: {},
          has_codex_credential: false,
          openai_login_available: true,
          openhub_model_options: [],
          openhub_available: false,
          openhub_wallet_url: "",
        }),
      });
    });
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantEmptyState(), { timeout: 60_000 })
      .toEqual({
        visible: true,
        addLabel: "Add connection",
      });
    await driver.cloudAssistantClickAdd();
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("add");
    await driver.page.unroute(CONNECTIONS_PATH);
  }
);

test(
  specTitle(["CLOUD-032"], "rows show kind-appropriate titles, markers, and controls"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Rows");
    const byok = await createByok(session.cookie);
    await cloudStampConnectionVerified(byok.id);
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 }).toHaveLength(2);

    const cards = await driver.cloudAssistantConnections();
    const account = cards.find((card) => card.title === "OpenAI account");
    const key = cards.find((card) => card.title === "BYOK:openrouter.ai");
    expect(account).toBeDefined();
    expect(key).toBeDefined();
    expect(account?.selected).toBe(false);
    expect(account?.selectDisabled).toBe(true);
    expect(account?.markers).toContain("Not connected");
    expect(account?.modelControl).toBe("hidden");
    expect(account?.actions).toContain("Continue with ChatGPT");
    expect(key?.selected).toBe(true);
    expect(key?.selectDisabled).toBe(false);
    expect(key?.markers).toContain("In use");
    // NEWPIDASH-394 (DIFF-65): the new app edits a key's model-ID list
    // in place instead of a free-text field; per-ID tests live on each
    // row, so the header carries no row-level Test.
    expect(key?.modelControl).toBe(driver.target === "web_new" ? "models" : "input");
    expect(key?.modelValue).toBe("meta-llama/llama-3.3-70b-instruct");
    expect(key?.modelValues).toEqual(["meta-llama/llama-3.3-70b-instruct"]);
    expect(key?.verifiedVisible).toBe(true);
    if (driver.target === "web_new") {
      expect(key?.actions).toContain("Remove");
      expect(key?.actions).not.toContain("Test");
    } else {
      expect(key?.actions).toContain("Test");
      expect(key?.actions).toContain("Remove");
    }

    // Server state: the list carries both rows with matching selection.
    const list = await cloudAssistantConnectionsRaw(session.cookie);
    expect(list.status).toBe(200);
    const rows = rowsOf(list.payload);
    expect(rows).toHaveLength(2);
    expect(rows.find((row) => row.kind === "codex")?.is_selected).toBe(false);
    expect(rows.find((row) => row.kind === "byok")?.is_selected).toBe(true);
  }
);

test(
  specTitle(["CLOUD-032"], "switching connections confirms and flips the server"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }, testInfo) => {
    // NEWPIDASH-394 (DIFF-65) removed the per-row select control from the
    // new app: choice moved per surface into the Models section, covered
    // by "the Models section switches a surface" below. This leg pins the
    // oracle's radio, which stays for older desktop builds.
    test.skip(
      testInfo.project.name !== "oracle",
      "per-row select is oracle-only since NEWPIDASH-394; the new app chooses per surface"
    );
    const email = uniqueEmail("parity-440-conn");
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Switch", email);
    await createByok(session.cookie);
    await cloudPlantCodexCredential(email);
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 }).toHaveLength(2);

    await driver.cloudAssistantSelectConnection("OpenAI account");
    await expect.poll(() => driver.toastText(), { timeout: 60_000 }).toContain("Switched");
    let list = await cloudAssistantConnectionsRaw(session.cookie);
    expect(rowsOf(list.payload).find((row) => row.kind === "codex")?.is_selected).toBe(true);

    await driver.cloudAssistantSelectConnection("BYOK:openrouter.ai");
    await expect.poll(() => driver.toastText(), { timeout: 60_000 }).toContain("Switched");
    list = await cloudAssistantConnectionsRaw(session.cookie);
    expect(rowsOf(list.payload).find((row) => row.kind === "byok")?.is_selected).toBe(true);
  }
);

const SELECTION_PATH = "**/api/users/me/ai-assistant/model-selection/**";
const BYOK_ID = "123e4567-e89b-12d3-a456-426614174001";

type SelectionState = {
  saved: { model: string; connection_id: string | null } | null;
  effective: { model: string; connection_id: string | null } | null;
  fallback_reason: string | null;
};

/**
 * Stub the per-surface selection API at the browser boundary (the
 * NEWPIDASH-392 backend is still landing): the stub records PUT writes
 * and answers the written state, so the new-app legs prove the Models
 * section's contract without a live seam.
 */
async function stubModelSelection(
  driver: { page: import("@playwright/test").Page },
  saved: Record<string, SelectionState>
): Promise<{ puts: Array<{ surface: string; body: unknown }> }> {
  const puts: Array<{ surface: string; body: unknown }> = [];
  await driver.page.route(SELECTION_PATH, async (route) => {
    const request = route.request();
    const pathname = new URL(request.url()).pathname;
    const surface = pathname
      .split("/")
      .filter((part) => part !== "")
      .pop();
    if (request.method() === "GET" && pathname === "/api/users/me/ai-assistant/model-selection/") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(saved),
      });
      return;
    }
    if (request.method() === "PUT") {
      const body = (request.postDataJSON() ?? {}) as { model?: unknown; connection_id?: unknown };
      const state: SelectionState = {
        saved: { model: String(body.model ?? ""), connection_id: (body.connection_id as string | null) ?? null },
        effective: { model: String(body.model ?? ""), connection_id: (body.connection_id as string | null) ?? null },
        fallback_reason: null,
      };
      if (surface !== undefined && surface !== "model-selection") saved[surface] = state;
      puts.push({ surface: surface ?? "", body });
      await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(state) });
      return;
    }
    await route.fallback();
  });
  return { puts };
}

function stubConnectionsWithModels(driver: { page: import("@playwright/test").Page }, cookie: string): Promise<void> {
  return driver.page.route(CONNECTIONS_PATH, async (route) => {
    const request = route.request();
    if (request.method() !== "GET" || new URL(request.url()).pathname !== "/api/users/me/ai-assistant/connections/") {
      await route.fallback();
      return;
    }
    const live = await cloudAssistantConnectionsRaw(cookie);
    const base = live.payload as Record<string, unknown>;
    const connections = (base["connections"] as Record<string, unknown>[]).map((row) =>
      row["kind"] === "byok"
        ? { ...row, models: ["gpt-6", "gpt-6.2"], model_verified_at: { "gpt-6": "2026-11-23T00:00:00Z" } }
        : row
    );
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ...base,
        connections,
        openhub_model_options: ["openhub_free_llm", "deepseek/deepseek-v4-pro"],
        openhub_model_labels: { "deepseek/deepseek-v4-pro": "Deepest reasoning" },
      }),
    });
  });
}

test(
  specTitle(["CLOUD-032"], "the Models section switches a surface without touching the others"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }, testInfo) => {
    test.skip(
      testInfo.project.name !== "new",
      "per-surface choice is new-app-only (NEWPIDASH-394, DIFF-65); the oracle pins the per-row radio"
    );
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Surface");
    await createByok(session.cookie);
    const live = await cloudAssistantConnectionsRaw(session.cookie);
    const byokId = (rowsOf(live.payload).find((row) => row.kind === "byok")?.id ?? BYOK_ID) as string;
    await stubConnectionsWithModels(driver, session.cookie);
    const blank: SelectionState = {
      saved: null,
      effective: { model: "openhub_free_llm", connection_id: null },
      fallback_reason: "none_saved",
    };
    const selection: Record<string, SelectionState> = {
      web_assistant: { ...blank },
      desktop_assistant: { ...blank },
      agent: { ...blank, effective: null, fallback_reason: "lane_unavailable" },
    };
    const { puts } = await stubModelSelection(driver, selection);
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));

    // All three surfaces render with the catalog default effective; the
    // agent offers catalog models only.
    const pickers = await driver.cloudAssistantModelsSection();
    expect(pickers.map((picker) => picker.label)).toEqual(["Web assistant", "Desktop assistant", "Pi Dash Agent"]);
    expect(pickers.every((picker) => picker.fallback === null)).toBe(true);
    const webOptions = await driver.cloudAssistantModelsOptions("Web assistant");
    expect(webOptions).toEqual([
      "OpenHub free model (default)",
      "Deepest reasoning",
      "gpt-6 (BYOK · openrouter.ai)",
      "gpt-6.2 (BYOK · openrouter.ai)",
    ]);
    const agentOptions = await driver.cloudAssistantModelsOptions("Pi Dash Agent");
    expect(agentOptions.every((option) => !option.includes("BYOK"))).toBe(true);

    // Picking a BYOK model for the web surface writes only that surface.
    await driver.cloudAssistantModelsSelect("Web assistant", "gpt-6 (BYOK · openrouter.ai)");
    expect(puts).toEqual([{ surface: "web_assistant", body: { model: "gpt-6", connection_id: byokId } }]);
    await expect
      .poll(
        async () =>
          (await driver.cloudAssistantModelsSection()).find((picker) => picker.label === "Web assistant")?.value,
        {
          timeout: 60_000,
        }
      )
      .toContain("gpt-6");
    const desktop = (await driver.cloudAssistantModelsSection()).find((picker) => picker.label === "Desktop assistant");
    expect(desktop?.value).toContain("OpenHub free model");
    expect(desktop?.fallback).toBeNull();

    await driver.page.unroute(SELECTION_PATH);
    await driver.page.unroute(CONNECTIONS_PATH);
  }
);

test(
  specTitle(["CLOUD-032"], "the Models section names the fallback when a saved choice stops working"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "per-surface fallback is new-app-only (NEWPIDASH-394, DIFF-65)");
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Fallback");
    await createByok(session.cookie);
    await stubConnectionsWithModels(driver, session.cookie);
    const selection: Record<string, SelectionState> = {
      web_assistant: {
        saved: { model: "modelC", connection_id: null },
        effective: { model: "openhub_free_llm", connection_id: null },
        fallback_reason: "model_removed",
      },
      desktop_assistant: {
        saved: null,
        effective: { model: "openhub_free_llm", connection_id: null },
        fallback_reason: "none_saved",
      },
      agent: { saved: null, effective: null, fallback_reason: "lane_unavailable" },
    };
    await stubModelSelection(driver, selection);
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));

    const web = (await driver.cloudAssistantModelsSection()).find((picker) => picker.label === "Web assistant");
    expect(web?.fallback).toContain("modelC is no longer available");
    const desktop = (await driver.cloudAssistantModelsSection()).find((picker) => picker.label === "Desktop assistant");
    expect(desktop?.fallback).toBeNull();

    await driver.page.unroute(SELECTION_PATH);
    await driver.page.unroute(CONNECTIONS_PATH);
  }
);

test(
  specTitle(["CLOUD-032"], "disconnected account row cannot be selected"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Guard");
    await createByok(session.cookie);
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 }).toHaveLength(2);
    const account = (await driver.cloudAssistantConnections()).find((card) => card.title === "OpenAI account");
    expect(account?.selectDisabled).toBe(true);

    // Server state: selecting the unbacked row is refused, not recorded.
    const list = await cloudAssistantConnectionsRaw(session.cookie);
    const codexId = rowsOf(list.payload).find((row) => row.kind === "codex")?.id ?? "";
    expect(codexId).not.toBe("");
    const refused = await cloudAssistantSelectRaw(session.cookie, codexId);
    expect(refused.status).toBe(400);
    expect(refused.payload["error"]).toBe("codex_not_connected");
  }
);

test(
  specTitle(["CLOUD-032"], "model edits commit per control kind"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }, testInfo) => {
    // NEWPIDASH-394 (DIFF-65): the new app edits a key's ID list in place
    // and PATCHes the full list; the oracle commits a single free-text
    // value. The new-app flow is stubbed at the browser boundary below.
    test.skip(
      testInfo.project.name !== "oracle",
      "single-model commit is oracle-only since NEWPIDASH-394; the new app edits the ID list"
    );
    const email = uniqueEmail("parity-440-conn");
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Model", email);
    await createByok(session.cookie);
    await cloudPlantCodexCredential(email);
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 }).toHaveLength(2);

    const before = await driver.cloudAssistantConnections();
    const account = before.find((card) => card.title === "OpenAI account");
    expect(account?.modelControl).toBe("select");
    const offered = account?.modelOptions ?? [];
    expect(offered.length).toBeGreaterThan(1);

    const picked = offered.includes("gpt-5.5") ? "gpt-5.5" : (offered[offered.length - 1] ?? "");
    expect(picked).not.toBe("");
    await driver.cloudAssistantSetModel("OpenAI account", picked);
    await driver.cloudAssistantSetModel("BYOK:openrouter.ai", "llama-edited");
    await expect
      .poll(async () => rowsOf((await cloudAssistantConnectionsRaw(session.cookie)).payload), {
        timeout: 60_000,
      })
      .toEqual(
        expect.arrayContaining([
          expect.objectContaining({ kind: "codex", model_name: picked }),
          expect.objectContaining({ kind: "byok", model_name: "llama-edited" }),
        ])
      );
  }
);

test(
  specTitle(["CLOUD-032"], "the editor commits longer and shorter model lists"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "multi-model editing is new-app-only (NEWPIDASH-394, DIFF-65)");
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Editor");
    await createByok(session.cookie);
    const live = await cloudAssistantConnectionsRaw(session.cookie);
    const byokId = (rowsOf(live.payload).find((row) => row.kind === "byok")?.id ?? BYOK_ID) as string;

    // Stateful stub: PATCH writes replace the row's list; GET refetches
    // observe it, which is how the UI proves the commit landed.
    let models = ["gpt-6"];
    const patches: unknown[] = [];
    await driver.page.route(CONNECTIONS_PATH, async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (request.method() === "GET" && pathname === "/api/users/me/ai-assistant/connections/") {
        const base = (await cloudAssistantConnectionsRaw(session.cookie)).payload as Record<string, unknown>;
        const connections = (base["connections"] as Record<string, unknown>[]).map((row) =>
          row["kind"] === "byok" ? { ...row, models, model_verified_at: {} } : row
        );
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ ...base, connections }),
        });
        return;
      }
      if (request.method() === "PATCH" && pathname === `/api/users/me/ai-assistant/connections/${byokId}/`) {
        const body = (request.postDataJSON() ?? {}) as { models?: unknown };
        models = Array.isArray(body.models) ? (body.models as string[]) : models;
        patches.push(body);
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ ...(rowsOf(live.payload).find((row) => row.kind === "byok") ?? {}), models }),
        });
        return;
      }
      await route.fallback();
    });
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnectionModels("BYOK:openrouter.ai"), { timeout: 60_000 })
      .toEqual(["gpt-6"]);

    await driver.cloudAssistantConnectionAddModel("BYOK:openrouter.ai", "gpt-6.2");
    expect(patches).toEqual([{ models: ["gpt-6", "gpt-6.2"], save_unverified: false }]);
    await expect
      .poll(() => driver.cloudAssistantConnectionModels("BYOK:openrouter.ai"), { timeout: 60_000 })
      .toEqual(["gpt-6", "gpt-6.2"]);

    await driver.cloudAssistantConnectionRemoveModel("BYOK:openrouter.ai", "gpt-6");
    expect(patches).toEqual([
      { models: ["gpt-6", "gpt-6.2"], save_unverified: false },
      { models: ["gpt-6.2"], save_unverified: false },
    ]);
    await expect
      .poll(() => driver.cloudAssistantConnectionModels("BYOK:openrouter.ai"), { timeout: 60_000 })
      .toEqual(["gpt-6.2"]);

    await driver.page.unroute(CONNECTIONS_PATH);
  }
);

test(
  specTitle(["CLOUD-032"], "a refused write reports per ID with Save anyway for unverified-only refusals"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "per-ID save results are new-app-only (NEWPIDASH-394, DIFF-65)");
    const session = await cloudOnboardedUser("parity-440-conn", "Conn SaveAnyway");
    await createByok(session.cookie);
    const live = await cloudAssistantConnectionsRaw(session.cookie);
    const byokId = (rowsOf(live.payload).find((row) => row.kind === "byok")?.id ?? BYOK_ID) as string;

    const patches: unknown[] = [];
    let attempt = 0;
    await driver.page.route(CONNECTIONS_PATH, async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (request.method() === "GET" && pathname === "/api/users/me/ai-assistant/connections/") {
        const base = (await cloudAssistantConnectionsRaw(session.cookie)).payload as Record<string, unknown>;
        const connections = (base["connections"] as Record<string, unknown>[]).map((row) =>
          row["kind"] === "byok" ? { ...row, models: ["gpt-6"], model_verified_at: {} } : row
        );
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ ...base, connections }),
        });
        return;
      }
      if (request.method() === "PATCH" && pathname === `/api/users/me/ai-assistant/connections/${byokId}/`) {
        const body = request.postDataJSON() ?? {};
        patches.push(body);
        attempt += 1;
        if (attempt === 1) {
          await route.fulfill({
            status: 400,
            contentType: "application/json",
            body: JSON.stringify({
              error: "model_test_failed",
              models: [
                {
                  model: "flaky",
                  status: "unverified",
                  code: "provider_unreachable",
                  detail: "The provider timed out.",
                },
              ],
            }),
          });
          return;
        }
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ ...(rowsOf(live.payload).find((row) => row.kind === "byok") ?? {}) }),
        });
        return;
      }
      await route.fallback();
    });
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnectionModels("BYOK:openrouter.ai"), { timeout: 60_000 })
      .toEqual(["gpt-6"]);

    await driver.cloudAssistantConnectionAddModel("BYOK:openrouter.ai", "flaky");
    await expect
      .poll(() => driver.cloudAssistantConnectionModelDetail("BYOK:openrouter.ai", "flaky"), { timeout: 60_000 })
      .toBe("The provider timed out.");
    // Nothing saved: the refused list stays rendered with the retry.
    expect(await driver.cloudAssistantConnectionModels("BYOK:openrouter.ai")).toEqual(["gpt-6", "flaky"]);

    await driver.cloudAssistantConnectionSaveAnyway("BYOK:openrouter.ai");
    expect(patches).toEqual([
      { models: ["gpt-6", "flaky"], save_unverified: false },
      { models: ["gpt-6", "flaky"], save_unverified: true },
    ]);

    await driver.page.unroute(CONNECTIONS_PATH);
  }
);

test(
  specTitle(["CLOUD-032"], "testing the active connection reports failures inline"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }, testInfo) => {
    // NEWPIDASH-394 (DIFF-65): the new app tests one stored ID at a time
    // and reports the provider message beside it; the oracle's row-level
    // probe toasts. The new-app flow is stubbed at the browser boundary
    // below.
    test.skip(
      testInfo.project.name !== "oracle",
      "row-level test toast is oracle-only since NEWPIDASH-394; the new app reports per ID"
    );
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Test");
    await createByok(session.cookie, { baseUrl: "https://parity-440-dead.invalid/api/v1" });
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 })
      .toEqual(expect.arrayContaining([expect.objectContaining({ title: "BYOK:parity-440-dead.invalid" })]));

    await driver.cloudAssistantClickRowAction("BYOK:parity-440-dead.invalid", "Test");
    await expect.poll(() => driver.toastText(), { timeout: 120_000 }).toContain("Connection failed");

    // Server state: the test reports failure and stamps nothing verified.
    const test = await cloudAssistantTestRaw(session.cookie);
    expect(test.status).toBe(200);
    expect(test.payload["ok"]).toBe(false);
    const list = await cloudAssistantConnectionsRaw(session.cookie);
    const row = rowsOf(list.payload).find((entry) => entry.kind === "byok");
    expect(row).toBeDefined();
    expect((row as unknown as Record<string, unknown>)["last_verified_at"]).toBeNull();
  }
);

test(
  specTitle(["CLOUD-032"], "a failed per-ID test reports beside the ID without a toast"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "per-ID test results are new-app-only (NEWPIDASH-394, DIFF-65)");
    const session = await cloudOnboardedUser("parity-440-conn", "Conn PerIdTest");
    await createByok(session.cookie);
    const live = await cloudAssistantConnectionsRaw(session.cookie);
    const byokId = (rowsOf(live.payload).find((row) => row.kind === "byok")?.id ?? BYOK_ID) as string;

    await driver.page.route(CONNECTIONS_PATH, async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (request.method() === "GET" && pathname === "/api/users/me/ai-assistant/connections/") {
        const base = (await cloudAssistantConnectionsRaw(session.cookie)).payload as Record<string, unknown>;
        const connections = (base["connections"] as Record<string, unknown>[]).map((row) =>
          row["kind"] === "byok" ? { ...row, models: ["gpt-6"], model_verified_at: {} } : row
        );
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ ...base, connections }),
        });
        return;
      }
      if (request.method() === "POST" && pathname === `/api/users/me/ai-assistant/connections/${byokId}/test/`) {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            model: "gpt-6",
            status: "failed",
            code: "model_invalid",
            detail: "The provider refused gpt-6.",
          }),
        });
        return;
      }
      await route.fallback();
    });
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnectionModels("BYOK:openrouter.ai"), { timeout: 60_000 })
      .toEqual(["gpt-6"]);

    await driver.cloudAssistantConnectionTestModel("BYOK:openrouter.ai", "gpt-6");
    await expect
      .poll(() => driver.cloudAssistantConnectionModelDetail("BYOK:openrouter.ai", "gpt-6"), { timeout: 60_000 })
      .toBe("The provider refused gpt-6.");
    // The failure renders inline; no toast fires.
    await driver.page.waitForTimeout(2000);
    expect(await driver.toastText()).toBeNull();

    await driver.page.unroute(CONNECTIONS_PATH);
  }
);

test(
  specTitle(["CLOUD-032"], "removing the last model ID asks first, then deletes the key"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "last-ID removal is new-app-only (NEWPIDASH-394, DIFF-65)");
    const session = await cloudOnboardedUser("parity-440-conn", "Conn LastId");
    await createByok(session.cookie);
    const live = await cloudAssistantConnectionsRaw(session.cookie);
    const byokId = (rowsOf(live.payload).find((row) => row.kind === "byok")?.id ?? BYOK_ID) as string;

    let connections = (live.payload as Record<string, unknown>)["connections"] as Record<string, unknown>[];
    connections = connections.map((row) =>
      row["kind"] === "byok" ? { ...row, models: ["only-one"], model_verified_at: {} } : row
    );
    const deleted: string[] = [];
    await driver.page.route(CONNECTIONS_PATH, async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (request.method() === "GET" && pathname === "/api/users/me/ai-assistant/connections/") {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ ...(live.payload as Record<string, unknown>), connections }),
        });
        return;
      }
      if (request.method() === "DELETE" && pathname === `/api/users/me/ai-assistant/connections/${byokId}/`) {
        deleted.push(byokId);
        connections = connections.filter((row) => row["kind"] !== "byok");
        await route.fulfill({ status: 204 });
        return;
      }
      await route.fallback();
    });
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnectionModels("BYOK:openrouter.ai"), { timeout: 60_000 })
      .toEqual(["only-one"]);

    // The last ID asks for confirmation naming the key; confirming calls
    // DELETE and the card leaves with it.
    await driver.cloudAssistantConnectionRemoveModel("BYOK:openrouter.ai", "only-one");
    expect(deleted).toEqual([byokId]);
    await expect
      .poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 })
      .toEqual(expect.arrayContaining([expect.not.objectContaining({ title: "BYOK:openrouter.ai" })]));

    await driver.page.unroute(CONNECTIONS_PATH);
  }
);

test(
  specTitle(["CLOUD-032"], "removing clears self-supplied rows and disconnect keeps the account row"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }) => {
    const email = uniqueEmail("parity-440-conn");
    const session = await cloudOnboardedUser("parity-440-conn", "Conn Remove", email);
    await createByok(session.cookie);
    await cloudPlantCodexCredential(email);
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 }).toHaveLength(2);

    await driver.cloudAssistantClickRowAction("BYOK:openrouter.ai", "Remove");
    await expect.poll(() => driver.toastText(), { timeout: 60_000 }).toContain("Removed");
    await expect.poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 }).toHaveLength(1);
    let list = await cloudAssistantConnectionsRaw(session.cookie);
    expect(rowsOf(list.payload).some((row) => row.kind === "byok")).toBe(false);
    // Removing the selected row falls back to the remaining connected row.
    expect(rowsOf(list.payload).find((row) => row.kind === "codex")?.is_selected).toBe(true);

    await driver.cloudAssistantClickRowAction("OpenAI account", "Disconnect");
    await expect.poll(() => driver.toastText(), { timeout: 60_000 }).toContain("disconnected");
    const cards = await driver.cloudAssistantConnections();
    expect(cards).toHaveLength(1);
    expect(cards[0]?.title).toBe("OpenAI account");
    expect(cards[0]?.selected).toBe(false);
    expect(cards[0]?.markers).toContain("Not connected");

    list = await cloudAssistantConnectionsRaw(session.cookie);
    expect(list.payload["has_codex_credential"]).toBe(false);
    expect(rowsOf(list.payload)).toHaveLength(1);
  }
);
