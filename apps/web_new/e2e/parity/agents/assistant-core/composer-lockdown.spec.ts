// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): with no provider key stored, the thread
// composer locks down — inputs disable with an explanatory line — and no
// submit can reach the server, so keyless threads stay empty. (The chat
// root's key-required toast only fires on a mid-session key removal plus a
// config revalidation, which no deterministic UI path drives; the
// dashboard card proves the same toast in AGT-048.) Row: AGT-040.
import { test, expect } from "../../fixtures";
import { serverAssistantConfigGet, serverAssistantMessages, serverAssistantThreadCreate } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { API_KEY_REMINDER } from "./support";

const ROWS = ["AGT-040"];

test(
  specTitle(ROWS, "composer locks down without a provider key and nothing sends"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt40"));
    const { owner, workspaceSlug } = harness;

    await test.step("no provider key is stored", async () => {
      expect((await serverAssistantConfigGet(harness.ownerSession)).has_api_key).toBe(false);
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
    });

    await test.step("inputs disable with the explanatory line", async () => {
      await expect.poll(() => driver.assistantComposerReason(), { timeout: 60_000 }).toBe(API_KEY_REMINDER);
      expect(await driver.assistantTextareaDisabled()).toBe(true);
      expect(await driver.assistantSendEnabled()).toBe(false);
      expect(await driver.assistantStopVisible()).toBe(false);
    });

    await test.step("no submit reaches the server", async () => {
      await driver.assistantStartApiSpy();
      // Disabled inputs swallow submits: Enter and the send button do
      // nothing, so the spy must stay silent.
      await expect.poll(() => driver.assistantTextareaDisabled(), { timeout: 10_000 }).toBe(true);
      expect((await driver.assistantApiCounts()).send).toBe(0);
      await driver.assistantStopApiSpy();
      expect(await serverAssistantMessages(workspaceSlug, thread.id, harness.ownerSession)).toEqual([]);
      expect(await driver.assistantBubbles()).toEqual([]);
    });
  }
);
