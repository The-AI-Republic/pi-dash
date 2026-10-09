// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): provider and speech keys each live
// through read, save, remove and a connectivity check, and the chat and
// dictation gates react to the stored state. The settings-page chrome
// itself is rowed under profile/settings — only the gates surface here.
// (The flag-off 404 halves stay a Gap in the row: VOICE_DICTATION_ENABLED
// ships in PR #586, still open, and this checkout answers every dictation
// write unflagged.) Row: AGT-052.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigGet,
  serverAssistantConfigPut,
  serverAssistantConfigTest,
  serverAssistantSttDelete,
  serverAssistantSttGet,
  serverAssistantSttPut,
  serverAssistantSttTest,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { API_KEY_REMINDER, DUMMY_PROVIDER, DUMMY_STT } from "../assistant-core/support";

const ROWS = ["AGT-052"];

test(
  specTitle(ROWS, "both keys live through read, save, check and remove; gates react"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt52"));
    const { owner, workspaceSlug } = harness;

    await test.step("both keys start absent", async () => {
      expect((await serverAssistantConfigGet(harness.ownerSession)).has_api_key).toBe(false);
      expect((await serverAssistantSttGet(harness.ownerSession)).has_api_key).toBe(false);
    });

    await test.step("provider key saves, reads back, and checks", async () => {
      const saved = await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
      expect(saved.has_api_key).toBe(true);
      // The backend normalizes the URL (trailing slash stripped).
      expect(saved.base_url).toBe("http://127.0.0.1:9");
      const check = await serverAssistantConfigTest(harness.ownerSession);
      expect(check.ok).toBe(false);
      expect(check.error_code).toBe("provider_unreachable");
    });

    await test.step("speech key saves, reads back, and checks", async () => {
      const saved = await serverAssistantSttPut(harness.ownerSession, DUMMY_STT);
      expect(saved.has_api_key).toBe(true);
      expect(saved.model_name).toBe(DUMMY_STT.model_name);
      const check = await serverAssistantSttTest(harness.ownerSession);
      expect(check.ok).toBe(false);
      expect(check.error_code).toBe("provider_unreachable");
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and both gates read open", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
      expect(await driver.assistantComposerReason()).toBeNull();
      expect(await driver.assistantTextareaDisabled()).toBe(false);
      expect(await driver.assistantMicLabel()).toBe("Hold to dictate");
    });

    await test.step("speech key removal shuts only the dictation gate", async () => {
      await serverAssistantSttDelete(harness.ownerSession);
      expect((await serverAssistantSttGet(harness.ownerSession)).has_api_key).toBe(false);
      const keyless = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, keyless.id);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 60_000 }).toBe("Set up voice dictation");
      expect(await driver.assistantDictationHint()).toBe("Voice dictation isn't set up. Configure it.");
      expect(await driver.assistantTextareaDisabled()).toBe(false);
    });

    await test.step("provider key removal locks the chat gate down", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
      expect((await serverAssistantConfigGet(harness.ownerSession)).has_api_key).toBe(false);
      const locked = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, locked.id);
      await expect.poll(() => driver.assistantComposerReason(), { timeout: 60_000 }).toBe(API_KEY_REMINDER);
      expect(await driver.assistantTextareaDisabled()).toBe(true);
      await driver.assistantOpenLanding(workspaceSlug);
      await expect.poll(() => driver.assistantSetupCard(), { timeout: 60_000 }).not.toBeNull();
    });
  }
);
