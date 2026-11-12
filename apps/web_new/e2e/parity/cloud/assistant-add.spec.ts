// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): adding connections, row CLOUD-033
// (adding covers self-supplied keys and provider sign-in), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud profile area lands there.
//
// Observed behavior notes (cloud oracle): the key form requires a
// provider, a reachable base for custom endpoints, a model, and a secret
// before save enables; the other provider hides the base field. Saving
// posts trimmed values and lands back on the list with the new row
// selected. Provider sign-in reserves a popup tab during the click,
// points it at the provider authorize URL, and polls for completion —
// the provider itself is an external boundary, so completion is proven
// through the polling and the distinct blocked, failed, and expired
// reports. Where the deployment cannot support provider sign-in the path
// hides entirely; this stack supports it, so that half runs against a
// stubbed availability flag.
import { test, expect } from "../fixtures";
import {
  cloudAssistantConnectionsRaw,
  cloudAssistantCreateByokRaw,
  cloudBrowserCookies,
  cloudOpenaiLoginStatusRaw,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { cloudOnboardedUser } from "./cloud-user-setup";

const CONNECTIONS_PATH = "**/api/users/me/ai-assistant/connections/";
const STATUS_PATH = "**/api/users/me/ai-assistant/openai-login/status/*";

test(
  specTitle(["CLOUD-033"], "key form requires provider, base, model, and secret"),
  { tag: specTags(["CLOUD-033"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-add", "Add Form");
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("list");
    await driver.cloudAssistantClickAdd();
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("add");

    const initial = await driver.cloudAssistantAddFormFacts();
    expect(initial.providers).toEqual(["openai_compatible", "anthropic"]);
    expect(initial.baseVisible).toBe(true);
    expect(initial.saveEnabled).toBe(false);

    await driver.cloudAssistantAddFill({ model: "m", key: "k" });
    expect((await driver.cloudAssistantAddFormFacts()).saveEnabled).toBe(false);
    await driver.cloudAssistantAddFill({ baseUrl: "https://openrouter.ai/api/v1" });
    expect((await driver.cloudAssistantAddFormFacts()).saveEnabled).toBe(true);

    await driver.cloudAssistantAddFill({ provider: "anthropic" });
    const anthropic = await driver.cloudAssistantAddFormFacts();
    expect(anthropic.provider).toBe("anthropic");
    expect(anthropic.baseVisible).toBe(false);
    expect(anthropic.saveEnabled).toBe(true);
  }
);

test(
  specTitle(["CLOUD-033"], "saving stores a trimmed selected connection"),
  { tag: specTags(["CLOUD-033"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-add", "Add Save");
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("list");

    await driver.cloudAssistantClickAdd();
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("add");
    await driver.cloudAssistantAddFill({
      model: "  meta-llama/llama-3.3-70b-instruct  ",
      baseUrl: "https://openrouter.ai/api/v1",
      key: "  parity-440-add-key  ",
    });
    await driver.cloudAssistantAddSave();
    await expect.poll(() => driver.toastText(), { timeout: 60_000 }).toContain("Connection added");
    const cards = await driver.cloudAssistantConnections();
    const key = cards.find((card) => card.title === "BYOK:openrouter.ai");
    expect(key?.selected).toBe(true);

    let list = await cloudAssistantConnectionsRaw(session.cookie);
    const rows = list.payload["connections"] as Record<string, unknown>[];
    const stored = rows.find((row) => row["kind"] === "byok");
    expect(stored?.["model_name"]).toBe("meta-llama/llama-3.3-70b-instruct");
    expect(stored?.["base_url"]).toBe("https://openrouter.ai/api/v1");
    expect(stored?.["is_selected"]).toBe(true);

    await driver.cloudAssistantClickAdd();
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("add");
    await driver.cloudAssistantAddFill({ provider: "anthropic", model: "claude-opus", key: "parity-anthropic" });
    await driver.cloudAssistantAddSave();
    const again = await driver.cloudAssistantConnections();
    expect(again.find((card) => card.title === "BYOK:Anthropic")?.selected).toBe(true);
    list = await cloudAssistantConnectionsRaw(session.cookie);
    const anthropic = (list.payload["connections"] as Record<string, unknown>[]).find(
      (row) => row["provider_kind"] === "anthropic"
    );
    expect(anthropic?.["base_url"]).toBe("");
  }
);

test(
  specTitle(["CLOUD-033"], "unreachable bases report from the server"),
  { tag: specTags(["CLOUD-033"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-add", "Add Invalid");
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("list");
    const before = await cloudAssistantConnectionsRaw(session.cookie);
    const count = ((before.payload["connections"] as unknown[]) ?? []).length;

    await driver.cloudAssistantClickAdd();
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("add");
    await driver.cloudAssistantAddFill({ model: "m", baseUrl: "not-a-url", key: "k" });
    await driver.cloudAssistantAddSave();
    await expect.poll(() => driver.toastText(), { timeout: 60_000 }).toContain("Could not add connection");

    const after = await cloudAssistantConnectionsRaw(session.cookie);
    expect(((after.payload["connections"] as unknown[]) ?? []).length).toBe(count);
  }
);

test(
  specTitle(["CLOUD-033"], "provider sign-in opens the authorize popup and polls for completion"),
  { tag: specTags(["CLOUD-033"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-add", "Add Provider");
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 })
      .toEqual(expect.arrayContaining([expect.objectContaining({ title: "OpenAI account" })]));

    await driver.cloudAssistantArmStatusCounter();
    await driver.cloudAssistantClickProviderSignIn("OpenAI account");
    const popupUrl = await driver.cloudAssistantProviderPopupUrl();
    const authorize = new URL(popupUrl);
    expect(authorize.hostname).toContain("openai.com");
    const state = authorize.searchParams.get("state") ?? "";
    expect(state).not.toBe("");
    await expect.poll(() => driver.cloudAssistantStatusHits(), { timeout: 60_000 }).toBeGreaterThanOrEqual(1);

    // Server state: the started flow polls as pending.
    const status = await cloudOpenaiLoginStatusRaw(session.cookie, state);
    expect(status.status).toBe(200);
    expect(status.payload["status"]).toBe("pending");
  }
);

test(
  specTitle(["CLOUD-033"], "blocked provider popups report distinctly"),
  { tag: specTags(["CLOUD-033"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-add", "Add Blocked");
    await driver.cloudAssistantBlockPopups();
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 })
      .toEqual(expect.arrayContaining([expect.objectContaining({ title: "OpenAI account" })]));
    await driver.cloudAssistantClickProviderSignIn("OpenAI account");
    await expect.poll(() => driver.toastText(), { timeout: 60_000 }).toContain("Popup blocked");
  }
);

test(
  specTitle(["CLOUD-033"], "expired provider logins report distinctly"),
  { tag: specTags(["CLOUD-033"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-add", "Add Expired");
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 })
      .toEqual(expect.arrayContaining([expect.objectContaining({ title: "OpenAI account" })]));

    await driver.cloudAssistantArmStatusCounter();
    await driver.cloudAssistantClickProviderSignIn("OpenAI account");
    const popupUrl = await driver.cloudAssistantProviderPopupUrl();
    expect(new URL(popupUrl).hostname).toContain("openai.com");
    await driver.page.route(STATUS_PATH, async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "expired" }),
      });
    });
    await expect.poll(() => driver.toastText(), { timeout: 60_000 }).toContain("OpenAI sign-in failed");
    expect(await driver.cloudAssistantStatusHits()).toBeGreaterThanOrEqual(1);
    await driver.page.unroute(STATUS_PATH);
  }
);

test(
  specTitle(["CLOUD-033"], "provider sign-in hides where the deployment cannot support it"),
  { tag: specTags(["CLOUD-033"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-add", "Add Hidden");
    const created = await cloudAssistantCreateByokRaw(session.cookie, {
      provider_kind: "openai_compatible",
      base_url: "https://openrouter.ai/api/v1",
      model_name: "meta-llama/llama-3.3-70b-instruct",
      api_key: "parity-440-add-key",
    });
    expect(created.status).toBe(201);
    const live = await cloudAssistantConnectionsRaw(session.cookie);
    expect(live.status).toBe(200);
    expect(live.payload["openai_login_available"]).toBe(true);
    const base = live.payload as Record<string, unknown>;
    const rows = (base["connections"] as Record<string, unknown>[]).filter((row) => row["kind"] !== "codex");

    await driver.page.route(CONNECTIONS_PATH, async (route) => {
      const request = route.request();
      if (request.method() !== "GET" || new URL(request.url()).pathname !== "/api/users/me/ai-assistant/connections/") {
        await route.fallback();
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ ...base, connections: rows, openai_login_available: false }),
      });
    });
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 }).toHaveLength(rows.length);
    const texts = await driver.page.locator("main").first().innerText();
    expect(texts).not.toContain("Continue with ChatGPT");
    await driver.page.unroute(CONNECTIONS_PATH);
  }
);
