// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): pressing the composer mic without a
// stored speech key routes to dictation settings instead of recording,
// while a keyed press stays on the thread. (The flag-off hidden-control
// half stays a Gap in the row: VOICE_DICTATION_ENABLED ships in PR #586,
// still open, and this checkout has no flag.) Row: AGT-051.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantSttDelete,
  serverAssistantSttGet,
  serverAssistantSttPut,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, DUMMY_STT } from "../assistant-core/support";

const ROWS = ["AGT-051"];

test(
  specTitle(ROWS, "keyless mic press routes to dictation settings, keyed press stays"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt51"));
    const { owner, workspaceSlug } = harness;

    await test.step("owner stores a provider key and no speech key", async () => {
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

    await test.step("keyless press navigates to dictation settings", async () => {
      expect(await driver.assistantMicLabel()).toBe("Set up voice dictation");
      await driver.assistantClickMic();
      await expect
        .poll(() => driver.assistantCurrentPath(), { timeout: 30_000 })
        .toBe("/settings/profile/ai-assistant");
      expect(await driver.assistantCurrentHash()).toBe("#voice-dictation");
      expect((await serverAssistantSttGet(harness.ownerSession)).has_api_key).toBe(false);
    });

    await test.step("keyed press stays on the thread", async () => {
      await serverAssistantSttPut(harness.ownerSession, DUMMY_STT);
      expect((await serverAssistantSttGet(harness.ownerSession)).has_api_key).toBe(true);
      const keyed = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, keyed.id);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 60_000 }).toBe("Hold to dictate");
      await driver.assistantClickMic();
      // A click's fast up lands under the tap floor and discards, so no
      // navigation and no transcription follow; the denied/error hint the
      // headless stack raises is rowed under AGT-050.
      await expect
        .poll(() => driver.assistantCurrentPath(), { timeout: 30_000 })
        .toBe(`/${workspaceSlug}/assistant/${keyed.id}`);
      expect(await driver.assistantCurrentHash()).toBe("");
    });

    await test.step("both keys are restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
      await serverAssistantSttDelete(harness.ownerSession);
      expect((await serverAssistantSttGet(harness.ownerSession)).has_api_key).toBe(false);
    });
  }
);
