// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): with no provider key stored, a setup
// card replaces the chat entry on the landing and in a thread's empty
// state; it explains the user brings their own key and links to provider
// settings. Sends are refused server-side until a key exists. Row: AGT-039.
import { test, expect } from "../../fixtures";
import { serverAssistantConfigGet, serverAssistantSendRaw, serverAssistantThreadCreate } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-039"];

test(
  specTitle(ROWS, "setup card swaps in for chat entry until a provider key exists"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt39"));
    const { owner, workspaceSlug } = harness;

    await test.step("no provider key is stored", async () => {
      expect((await serverAssistantConfigGet(harness.ownerSession)).has_api_key).toBe(false);
    });

    await test.step("owner signs in and opens the landing", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenLanding(workspaceSlug);
    });

    await test.step("landing shows the setup card instead of greeting and composer", async () => {
      let card = null as Awaited<ReturnType<typeof driver.assistantSetupCard>>;
      await expect
        .poll(
          async () => {
            card = await driver.assistantSetupCard();
            return card !== null;
          },
          { timeout: 60_000 }
        )
        .toBe(true);
      expect(card?.title).toBe("Set up your AI assistant");
      expect(card?.body).toContain("Bring your own LLM provider key");
      expect(card?.body).toContain("exactly your permissions");
      expect(card?.button).toBe("Configure provider");
      expect(await driver.assistantLandingGreeting()).toBeNull();
      expect(await driver.assistantLandingComposerVisible()).toBe(false);
    });

    await test.step("the card links to provider settings", async () => {
      await driver.assistantSetupCardClick();
      await expect
        .poll(() => driver.assistantCurrentPath(), { timeout: 60_000 })
        .toBe("/settings/profile/ai-assistant");
    });

    await test.step("a thread empty state swaps in the same card", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantSetupCard(), { timeout: 60_000 }).not.toBeNull();
      expect(await driver.assistantBubbles()).toEqual([]);
    });

    await test.step("sends are refused until a key exists", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const refused = await serverAssistantSendRaw(workspaceSlug, thread.id, "hello", harness.ownerSession);
      expect(refused.status).toBe(422);
      expect((refused.payload as { error?: unknown }).error).toBe("llm_config_missing");
    });
  }
);
