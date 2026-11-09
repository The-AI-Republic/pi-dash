// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-system-only scenarios (NEWPIDASH-150): the cloud edition's assistant
// model picker badges the usage-meter-free models as Free and blocks paid
// models for free-plan users. Row CLOUD-041 (new-system-only). The old app
// provably lacks the free-quota picker, so every leg skips the oracle
// driver. All legs stub the free-quota proxy at the browser boundary —
// the backend proxy (PRIVATEPI1-137) is still landing, and the stub pins
// the picker's behavior to the contract shape, not to live data.
import { test, expect } from "../fixtures";
import { cloudAssistantConnectionsRaw, cloudBrowserCookies } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { cloudWorkspaceUser } from "./cloud-user-setup";

// `**` at the end, not `/`: the item PATCH lands on
// `.../connections/{id}/`, which a trailing-slash list pattern misses,
// letting the model commit escape to the live API (404 there).
const CONNECTIONS_PATH = "**/api/users/me/ai-assistant/connections/**";
const QUOTA_PATH = "**/api/openhub/usage/quota/";
const HOSTED_ID = "00000000-0000-4000-8000-0000000000aa";

function hostedPayload(base: Record<string, unknown>, over: Record<string, unknown>) {
  return {
    ...base,
    connections: [
      ...((base["connections"] as Record<string, unknown>[]) ?? []),
      {
        id: HOSTED_ID,
        kind: "openhub",
        provider_kind: "openhub",
        base_url: "",
        model_name: "deepseek/deepseek-v4-flash",
        has_api_key: false,
        is_selected: true,
        last_verified_at: null,
        created_at: new Date().toISOString(),
        connected: true,
        uses_chatgpt_plan: false,
      },
    ],
    openhub_model_options: ["deepseek/deepseek-v4-flash", "moonshotai/kimi-k3"],
    openhub_available: true,
    openhub_wallet_url: "https://hub.example.com/account/credits",
    ...over,
  };
}

async function stubQuota(driver: { page: import("@playwright/test").Page }, plan: string) {
  await driver.page.route(QUOTA_PATH, async (route) => {
    const request = route.request();
    if (request.method() !== "GET" || new URL(request.url()).pathname !== "/api/openhub/usage/quota/") {
      await route.fallback();
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        llm: { freeTokensUsed: 100, freeTokensLimit: 2_000_000, freeModels: ["deepseek/deepseek-v4-flash"] },
        resetsAt: "2026-11-10T00:00:00Z",
        plan,
      }),
    });
  });
}

async function stubConnections(driver: { page: import("@playwright/test").Page }, cookie: string) {
  const live = await cloudAssistantConnectionsRaw(cookie);
  expect(live.status).toBe(200);
  const base = live.payload as Record<string, unknown>;
  // Stateful stub: the hosted row exists only here, so the PATCH the
  // picker's commit issues must land here too. GET refetches after the
  // write see the picked model, which is how the UI observes the switch.
  let currentModel = "deepseek/deepseek-v4-flash";
  await driver.page.route(CONNECTIONS_PATH, async (route) => {
    const request = route.request();
    const pathname = new URL(request.url()).pathname;
    if (request.method() === "GET" && pathname === "/api/users/me/ai-assistant/connections/") {
      const payload = hostedPayload(base, {});
      const hosted = (payload["connections"] as Record<string, unknown>[]).find((row) => row["kind"] === "openhub");
      if (hosted !== undefined) hosted["model_name"] = currentModel;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(payload),
      });
      return;
    }
    if (request.method() === "PATCH" && pathname === `/api/users/me/ai-assistant/connections/${HOSTED_ID}/`) {
      const body = (request.postDataJSON() ?? {}) as { model_name?: unknown };
      if (typeof body.model_name === "string" && body.model_name !== "") currentModel = body.model_name;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          id: HOSTED_ID,
          kind: "openhub",
          provider_kind: "openhub",
          base_url: "",
          model_name: currentModel,
          has_api_key: false,
          is_selected: true,
          last_verified_at: null,
          created_at: new Date().toISOString(),
        }),
      });
      return;
    }
    await route.fallback();
  });
}

test(
  specTitle(["CLOUD-041"], "free-plan users see Free vs Needs-credit and cannot pick a paid model"),
  { tag: specTags(["CLOUD-041"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "the model budget pins the web_new cloud build only");
    const session = await cloudWorkspaceUser(seed, "parity-150-budget", 15, "Free Budget");
    await stubQuota(driver as never, "free");
    await stubConnections(driver as never, session.cookie);
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));

    await expect
      .poll(() => driver.cloudAssistantModelBudget("OpenHub"), { timeout: 60_000 })
      .toMatchObject({ value: "deepseek/deepseek-v4-flash" });
    // The quota read annotates the picker after first paint; wait for
    // the blocked state rather than reading the pre-quota unbadged list.
    await expect
      .poll(
        () =>
          driver
            .cloudAssistantModelBudget("OpenHub")
            .then((facts) => facts.options.find((option) => option.value === "moonshotai/kimi-k3")?.disabled),
        { timeout: 60_000 }
      )
      .toBe(true);
    const budget = await driver.cloudAssistantModelBudget("OpenHub");
    expect(budget.options).toHaveLength(2);
    const freeOption = budget.options.find((option) => option.value === "deepseek/deepseek-v4-flash");
    const paidOption = budget.options.find((option) => option.value === "moonshotai/kimi-k3");
    expect(freeOption?.text).toContain("Free");
    expect(freeOption?.disabled).toBe(false);
    expect(paidOption?.text).toContain("Needs credit");
    expect(paidOption?.disabled).toBe(true);
    expect(budget.notice).toBeNull();
    // The top-up link only renders with the stranded-model warning.
    expect(budget.topUpHref).toBeNull();
    await driver.page.unroute(QUOTA_PATH);
    await driver.page.unroute(CONNECTIONS_PATH);
  }
);

test(
  specTitle(["CLOUD-041"], "paid-plan users can pick every model with Free vs Uses-credit hints"),
  { tag: specTags(["CLOUD-041"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "the model budget pins the web_new cloud build only");
    const session = await cloudWorkspaceUser(seed, "parity-150-budget", 15, "Paid Budget");
    await stubQuota(driver as never, "payg");
    await stubConnections(driver as never, session.cookie);
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));

    await expect
      .poll(() => driver.cloudAssistantModelBudget("OpenHub"), { timeout: 60_000 })
      .toMatchObject({ value: "deepseek/deepseek-v4-flash" });
    // Wait for the quota annotations (pre-quota the list is unbadged).
    await expect
      .poll(
        () =>
          driver
            .cloudAssistantModelBudget("OpenHub")
            .then((facts) => facts.options.find((option) => option.value === "moonshotai/kimi-k3")?.text ?? ""),
        { timeout: 60_000 }
      )
      .toContain("Uses credit");
    let budget = await driver.cloudAssistantModelBudget("OpenHub");
    expect(budget.options.find((option) => option.value === "deepseek/deepseek-v4-flash")?.text).toContain("Free");
    const paidOption = budget.options.find((option) => option.value === "moonshotai/kimi-k3");
    expect(paidOption?.text).toContain("Uses credit");
    expect(paidOption?.disabled).toBe(false);
    // The top-up link only renders with the stranded-model warning.
    expect(budget.topUpHref).toBeNull();

    await driver.cloudAssistantSetModel("OpenHub", "moonshotai/kimi-k3");
    await expect
      .poll(() => driver.cloudAssistantModelBudget("OpenHub").then((facts) => facts.value), { timeout: 60_000 })
      .toBe("moonshotai/kimi-k3");
    budget = await driver.cloudAssistantModelBudget("OpenHub");
    expect(budget.value).toBe("moonshotai/kimi-k3");
    expect(budget.notice).toBeNull();
    await driver.page.unroute(QUOTA_PATH);
    await driver.page.unroute(CONNECTIONS_PATH);
  }
);

test(
  specTitle(["CLOUD-041"], "a free user already on a paid model is warned with a top-up link"),
  { tag: specTags(["CLOUD-041"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "the model budget pins the web_new cloud build only");
    const session = await cloudWorkspaceUser(seed, "parity-150-budget", 15, "Stranded Budget");
    await stubQuota(driver as never, "free");
    const live = await cloudAssistantConnectionsRaw(session.cookie);
    const base = live.payload as Record<string, unknown>;
    const stranded = hostedPayload(base, {});
    const hostedRow = (stranded["connections"] as Record<string, unknown>[]).find((row) => row["kind"] === "openhub")!;
    hostedRow["model_name"] = "moonshotai/kimi-k3";
    await driver.page.route(CONNECTIONS_PATH, async (route) => {
      const request = route.request();
      if (request.method() !== "GET" || new URL(request.url()).pathname !== "/api/users/me/ai-assistant/connections/") {
        await route.fallback();
        return;
      }
      await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(stranded) });
    });
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));

    await expect
      .poll(() => driver.cloudAssistantModelBudget("OpenHub").then((facts) => facts.notice), { timeout: 60_000 })
      .toContain("needs credit");
    const budget = await driver.cloudAssistantModelBudget("OpenHub");
    expect(budget.value).toBe("moonshotai/kimi-k3");
    expect(budget.topUpHref).toBe("https://hub.example.com/account/credits");
    await driver.page.unroute(QUOTA_PATH);
    await driver.page.unroute(CONNECTIONS_PATH);
  }
);
