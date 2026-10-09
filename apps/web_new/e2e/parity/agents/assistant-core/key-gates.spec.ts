// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): the provider key gates chat while the
// speech key gates dictation, and each missing prerequisite blocks only
// its own surface — chat sends with no speech key stored, and dictation
// reads ready with no provider key stored. (The VOICE_DICTATION_ENABLED
// thirds stay a Gap in the row: the flag ships in PR #586, still open,
// and this checkout has no flag.) Row: AGT-057.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantPollTerminal,
  serverAssistantSttDelete,
  serverAssistantSttGet,
  serverAssistantSttPut,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { API_KEY_REMINDER, DUMMY_PROVIDER, DUMMY_STT, expectBubbles } from "./support";

const ROWS = ["AGT-057"];

test(
  specTitle(ROWS, "provider key gates chat only, speech key gates dictation only"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt57"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key and no speech key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
      expect((await serverAssistantSttGet(harness.ownerSession)).has_api_key).toBe(false);
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
    });

    await test.step("chat sends while dictation reads unconfigured", async () => {
      expect(await driver.assistantMicLabel()).toBe("Set up voice dictation");
      expect(await driver.assistantDictationHint()).toBe("Voice dictation isn't set up. Configure it.");
      await driver.assistantFillDraft(`gated chat ${tag}`);
      await driver.assistantPressEnter();
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 2);
      expect(bubbles[0]).toEqual({ role: "user", text: `gated chat ${tag}` });
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
    });

    await test.step("owner swaps to a speech key with no provider key", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
      await serverAssistantSttPut(harness.ownerSession, DUMMY_STT);
      expect((await serverAssistantSttGet(harness.ownerSession)).has_api_key).toBe(true);
    });

    await test.step("chat locks down while dictation reads ready", async () => {
      const locked = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, locked.id);
      await expect.poll(() => driver.assistantComposerReason(), { timeout: 60_000 }).toBe(API_KEY_REMINDER);
      expect(await driver.assistantTextareaDisabled()).toBe(true);
      expect(await driver.assistantMicLabel()).toBe("Hold to dictate");
      expect(await driver.assistantDictationHint()).toBeNull();
      await driver.assistantOpenLanding(workspaceSlug);
      await expect.poll(() => driver.assistantSetupCard(), { timeout: 60_000 }).not.toBeNull();
    });

    await test.step("both keys are restored to absent", async () => {
      await serverAssistantSttDelete(harness.ownerSession);
      expect((await serverAssistantSttGet(harness.ownerSession)).has_api_key).toBe(false);
    });
  }
);
