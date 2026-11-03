// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): credential-lane awareness, row
// CLOUD-037 (assistant entry points know which credential lane is
// active), green on the cloud-edition old app first. The same file must
// go green on apps/web_new once the cloud profile area lands there.
//
// Observed behavior notes (cloud oracle): the provider-config endpoint
// reports the active lane — self-supplied, account, or hosted — plus one
// credential flag per lane, and the shared hook reports that lane with
// whether setup is still needed for it, defaulting to the self-supplied
// lane when the field predates the backend. Assistant entries show the
// setup card while setup is needed and the greeting with its composer
// once the lane carries a credential. Only the self-supplied lane is
// reachable live; the account lane runs through a planted credential
// and the hosted lane through a stubbed config read.
import { test, expect } from "../fixtures";
import {
  cloudAssistantConfigRaw,
  cloudAssistantConnectionsRaw,
  cloudAssistantCreateByokRaw,
  cloudAssistantSelectRaw,
  cloudBrowserCookies,
  cloudDevLogin,
  cloudPlantCodexCredential,
  uniqueEmail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { cloudWorkspaceUser } from "./cloud-user-setup";

const CONFIG_PATH = "**/api/users/me/ai-assistant/config/";

function laneOf(payload: Record<string, unknown>): string {
  return typeof payload["credential_mode"] === "string" ? payload["credential_mode"] : "";
}

test(
  specTitle(["CLOUD-037"], "provider-config reports the lane across credential states"),
  { tag: specTags(["CLOUD-037"]) },
  async () => {
    const email = uniqueEmail("parity-440-lane");
    const session = await cloudDevLogin(email, "Lane States");

    const fresh = await cloudAssistantConfigRaw(session.cookie);
    expect(fresh.status).toBe(200);
    expect(laneOf(fresh.payload)).toBe("byok");
    expect(fresh.payload["has_byok_api_key"]).toBe(false);
    expect(fresh.payload["has_codex_token"]).toBe(false);
    expect(fresh.payload["has_openhub_access"]).toBe(false);

    const created = await cloudAssistantCreateByokRaw(session.cookie, {
      provider_kind: "openai_compatible",
      base_url: "https://openrouter.ai/api/v1",
      model_name: "meta-llama/llama-3.3-70b-instruct",
      api_key: "parity-440-lane-key",
    });
    expect(created.status).toBe(201);
    const keyed = await cloudAssistantConfigRaw(session.cookie);
    expect(laneOf(keyed.payload)).toBe("byok");
    expect(keyed.payload["has_byok_api_key"]).toBe(true);

    await cloudPlantCodexCredential(email);
    const list = await cloudAssistantConnectionsRaw(session.cookie);
    const rows = list.payload["connections"] as { id: string; kind: string }[];
    const codexId = rows.find((row) => row.kind === "codex")?.id ?? "";
    expect(codexId).not.toBe("");
    const selected = await cloudAssistantSelectRaw(session.cookie, codexId);
    expect(selected.status).toBe(200);
    const account = await cloudAssistantConfigRaw(session.cookie);
    expect(laneOf(account.payload)).toBe("codex");
    expect(account.payload["has_codex_token"]).toBe(true);
  }
);

test(
  specTitle(["CLOUD-037"], "assistant entries gate on setup until the lane carries a credential"),
  { tag: specTags(["CLOUD-037"]) },
  async ({ driver, seed }) => {
    const session = await cloudWorkspaceUser(seed, "parity-440-lane", 15, "Lane Gate");
    await driver.openAuthenticated(`/${seed.workspaceSlug}/assistant`, cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.assistantSetupCard(), { timeout: 60_000 }).not.toBeNull();
    expect(await driver.assistantLandingGreeting()).toBeNull();

    const created = await cloudAssistantCreateByokRaw(session.cookie, {
      provider_kind: "openai_compatible",
      base_url: "https://openrouter.ai/api/v1",
      model_name: "meta-llama/llama-3.3-70b-instruct",
      api_key: "parity-440-lane-key",
    });
    expect(created.status).toBe(201);
    await driver.page.reload();
    await expect.poll(() => driver.assistantLandingGreeting(), { timeout: 60_000 }).not.toBeNull();
    expect(await driver.assistantSetupCard()).toBeNull();
    expect(await driver.assistantLandingComposerVisible()).toBe(true);
  }
);

test(
  specTitle(["CLOUD-037"], "hook keys off the active lane and defaults to self-supplied"),
  { tag: specTags(["CLOUD-037"]) },
  async ({ driver, seed }) => {
    const session = await cloudWorkspaceUser(seed, "parity-440-lane", 15, "Lane Hook");
    const live = await cloudAssistantConfigRaw(session.cookie);
    expect(live.status).toBe(200);
    const base = live.payload as Record<string, unknown>;

    // Hosted lane ready without any self-supplied key: the entry is ready.
    const hosted = { ...base, credential_mode: "openhub", has_openhub_access: true, has_byok_api_key: false };
    await driver.page.route(CONFIG_PATH, async (route) => {
      if (new URL(route.request().url()).pathname !== "/api/users/me/ai-assistant/config/") {
        await route.fallback();
        return;
      }
      await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(hosted) });
    });
    await driver.openAuthenticated(`/${seed.workspaceSlug}/assistant`, cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.assistantLandingGreeting(), { timeout: 60_000 }).not.toBeNull();
    expect(await driver.assistantSetupCard()).toBeNull();

    // Older backend without the lane fields: self-supplied default, setup needed.
    const legacy = {
      provider_kind: "openai_compatible",
      base_url: "",
      model_name: "",
      has_api_key: false,
      last_verified_at: null,
    };
    await driver.page.unroute(CONFIG_PATH);
    await driver.page.route(CONFIG_PATH, async (route) => {
      if (new URL(route.request().url()).pathname !== "/api/users/me/ai-assistant/config/") {
        await route.fallback();
        return;
      }
      await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(legacy) });
    });
    await driver.page.reload();
    await expect.poll(() => driver.assistantSetupCard(), { timeout: 60_000 }).not.toBeNull();
    await driver.page.unroute(CONFIG_PATH);
  }
);
