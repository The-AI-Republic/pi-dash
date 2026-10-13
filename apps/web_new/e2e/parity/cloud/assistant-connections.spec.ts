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
    expect(key?.modelControl).toBe("input");
    expect(key?.modelValue).toBe("meta-llama/llama-3.3-70b-instruct");
    expect(key?.verifiedVisible).toBe(true);
    expect(key?.actions).toContain("Test");
    expect(key?.actions).toContain("Remove");

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
  async ({ driver }) => {
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
  async ({ driver }) => {
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
  specTitle(["CLOUD-032"], "testing the active connection reports failures inline"),
  { tag: specTags(["CLOUD-032"]) },
  async ({ driver }) => {
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
