// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): missing capture hardware and a
// capture-less browser each get their honest branch — a specific
// next-step hint for the first, a hidden mic for the last — and chat
// keeps sending with the mic gone. (The denied-permission branch lives
// in dictation-denied.spec.ts: it needs a fake device without the
// auto-grant flag.) Row: AGT-050.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantPollTerminal,
  serverAssistantSttDelete,
  serverAssistantSttPut,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, DUMMY_STT, expectBubbles } from "../assistant-core/support";

const ROWS = ["AGT-050"];

test(
  specTitle(ROWS, "missing and unsupported capture branch honestly"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt50s"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores dummy provider and speech keys", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
      await serverAssistantSttPut(harness.ownerSession, DUMMY_STT);
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 60_000 }).toBe("Hold to dictate");
    });

    await test.step("missing hardware names the missing mic", async () => {
      await driver.assistantSetMicrophonePermission("granted");
      const nodevice = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, nodevice.id);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 60_000 }).toBe("Hold to dictate");
      await driver.assistantMicDown();
      await expect.poll(() => driver.assistantDictationHint(), { timeout: 60_000 }).toBe("No microphone found.");
      await driver.assistantMicUp();
    });

    await test.step("unsupported browsers hide the mic while chat sends", async () => {
      await driver.assistantSimulateUnsupportedCapture();
      const plain = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, plain.id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
      expect(await driver.assistantMicLabel()).toBeNull();
      expect(await driver.assistantDictationHint()).toBeNull();
      await driver.assistantFillDraft(`still chats ${tag}`);
      await driver.assistantPressEnter();
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 2);
      expect(bubbles[0]).toEqual({ role: "user", text: `still chats ${tag}` });
      await serverAssistantPollTerminal(workspaceSlug, plain.id, harness.ownerSession);
    });

    await test.step("both keys are restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
      await serverAssistantSttDelete(harness.ownerSession);
    });
  }
);
