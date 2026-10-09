// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): with capture hardware present but
// permission withheld, the press fails without recording or uploading,
// and a denial rejection maps to the browser-fix hint. (Headless prompts
// never surface a real NotAllowedError, so the denial mapping runs
// against the spec's rejection name, disclosed here. Grant-then-record
// needs the auto-grant flag and lives in dictation.spec.ts.) Row: AGT-050.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantSttDelete,
  serverAssistantSttPut,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, DUMMY_STT } from "../assistant-core/support";

const ROWS = ["AGT-050"];

// Fake capture hardware WITHOUT the auto-grant flag: permission prompts
// resolve against the default (withheld) policy instead of auto-granting.
test.use({
  launchOptions: {
    args: ["--use-fake-device-for-media-stream"],
  },
});

test(
  specTitle(ROWS, "withheld permission fails without upload; denial maps to the fix"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt50d"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores dummy provider and speech keys", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
      await serverAssistantSttPut(harness.ownerSession, DUMMY_STT);
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantSetMicrophonePermission("denied");
      await driver.assistantStubTranscribeText(`granted ${tag}`);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 60_000 }).toBe("Hold to dictate");
    });

    await test.step("withheld permission fails without recording or uploading", async () => {
      await driver.assistantMicDown();
      await expect
        .poll(() => driver.assistantDictationHint(), { timeout: 60_000 })
        .toBe("Couldn't access the microphone.");
      await driver.assistantMicUp();
      expect((await driver.assistantTranscribeRequests()).length).toBe(0);
    });

    await test.step("a denial rejection maps to the browser-fix hint", async () => {
      await driver.assistantSimulateMicDenial();
      const denied = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, denied.id);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 60_000 }).toBe("Hold to dictate");
      await driver.assistantMicDown();
      await expect
        .poll(() => driver.assistantDictationHint(), { timeout: 60_000 })
        .toBe("Microphone access is blocked. Enable it in your browser's site settings, then try again.");
      await driver.assistantMicUp();
      expect((await driver.assistantTranscribeRequests()).length).toBe(0);
      // A second denied press retries into the same hint, never a crash.
      await driver.assistantMicDown();
      await expect
        .poll(() => driver.assistantDictationHint(), { timeout: 60_000 })
        .toBe("Microphone access is blocked. Enable it in your browser's site settings, then try again.");
      await driver.assistantMicUp();
      await driver.assistantClearTranscribeStubs();
    });

    await test.step("both keys are restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
      await serverAssistantSttDelete(harness.ownerSession);
    });
  }
);
