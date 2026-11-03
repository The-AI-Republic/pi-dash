// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): keyboard operation in this area, row
// CLOUD-039 (no custom shortcuts; native behavior everywhere), green on
// the cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud profile area lands there.
//
// Observed behavior notes (cloud oracle): every action in this area is
// an ordinary reachable control — radios, pickers, buttons — text
// fields commit on Enter, and nothing requires app-specific keys. The
// walks below drive the area with Tab and Enter alone, asserting focus
// lands where each step needs it and the outcomes match the pointer
// paths proven per row.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import { cloudAssistantConnectionsRaw, cloudAssistantCreateByokRaw, cloudBrowserCookies } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { cloudOnboardedUser } from "./cloud-user-setup";

/** Tab until the focused control reads `label`, or fail after `limit` stops. */
async function tabTo(driver: ParityDriver, label: string, limit = 80): Promise<void> {
  for (let i = 0; i < limit; i++) {
    const focused = await driver.cloudFocusedControl();
    if (focused?.label === label) return;
    await driver.page.keyboard.press("Tab");
  }
  const focused = await driver.cloudFocusedControl();
  expect(focused?.label).toBe(label);
}

test(
  specTitle(["CLOUD-039"], "adding a connection works with Tab and Enter alone"),
  { tag: specTags(["CLOUD-039"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-keys", "Keys Add");
    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("list");

    await tabTo(driver, "Add connection");
    await driver.page.keyboard.press("Enter");
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("add");

    await tabTo(driver, "Base URL");
    await driver.page.keyboard.type("https://openrouter.ai/api/v1", { delay: 5 });
    await tabTo(driver, "Model");
    await driver.page.keyboard.type("meta-llama/llama-3.3-70b-instruct", { delay: 5 });
    await tabTo(driver, "API key");
    await driver.page.keyboard.type("parity-440-keys-key", { delay: 5 });
    await tabTo(driver, "Add connection");
    expect((await driver.cloudAssistantAddFormFacts()).saveEnabled).toBe(true);
    await driver.page.keyboard.press("Enter");
    await expect.poll(() => driver.cloudAssistantView(), { timeout: 60_000 }).toBe("list");

    // Server state: the keyboard-driven save stored the trimmed row, selected.
    const list = await cloudAssistantConnectionsRaw(session.cookie);
    const rows = (list.payload["connections"] as Record<string, unknown>[]) ?? [];
    const stored = rows.find((row) => row["kind"] === "byok");
    expect(stored?.["model_name"]).toBe("meta-llama/llama-3.3-70b-instruct");
    expect(stored?.["is_selected"]).toBe(true);
  }
);

test(
  specTitle(["CLOUD-039"], "model text commits on Enter and row actions fire from the keyboard"),
  { tag: specTags(["CLOUD-039"]) },
  async ({ driver }) => {
    const session = await cloudOnboardedUser("parity-440-keys", "Keys Enter");
    const created = await cloudAssistantCreateByokRaw(session.cookie, {
      provider_kind: "openai_compatible",
      base_url: "https://openrouter.ai/api/v1",
      model_name: "meta-llama/llama-3.3-70b-instruct",
      api_key: "parity-440-keys-key",
    });
    expect(created.status).toBe(201);

    await driver.openAuthenticated("/settings/profile/ai-assistant", cloudBrowserCookies(session.cookie));
    await expect
      .poll(() => driver.cloudAssistantConnections(), { timeout: 60_000 })
      .toEqual(expect.arrayContaining([expect.objectContaining({ title: "BYOK:openrouter.ai" })]));
    await tabTo(driver, "Model");
    await driver.page.keyboard.press("ControlOrMeta+a");
    await driver.page.keyboard.type("llama-keyboard", { delay: 5 });
    await driver.page.keyboard.press("Enter");
    await expect
      .poll(
        async () => {
          const list = await cloudAssistantConnectionsRaw(session.cookie);
          const rows = (list.payload["connections"] as Record<string, unknown>[]) ?? [];
          return rows.find((row) => row["kind"] === "byok")?.["model_name"] ?? "";
        },
        { timeout: 60_000 }
      )
      .toBe("llama-keyboard");

    // Row actions are ordinary buttons: Tab to Test and fire it with Enter.
    await tabTo(driver, "Test");
    await driver.page.keyboard.press("Enter");
    await expect.poll(() => driver.toastText(), { timeout: 120_000 }).not.toBeNull();
  }
);
