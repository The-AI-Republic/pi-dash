// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-892): the operator's voice-dictation kill
// switch (main#586, restated in §14 of the agents spec). The switch is
// instance-wide and travels on the speech-config read the app already
// makes: with it off the composer shows no microphone or hint, a recording
// in progress is abandoned when the tab refocuses and sees the flip, and a
// dictation-disabled answer re-reads the config so the control hides. The
// speech save, connection test and transcription upload answer not-found
// with the disabled code, while removing a saved key stays open and the
// read keeps reporting the flag.
//
// Target note: this checkout's backend predates VOICE_DICTATION_ENABLED, so
// the oracle leg can only prove "flag absent ⇒ the ordinary dictation UI".
// The flag-off halves are target-only: the driver serves the flag on the
// read and the disabled refusals at the browser boundary, and every such
// branch is guarded by `driver.target === "web_new"` and named in a step.
// Row: AGT-064.
import { test, expect } from "../../fixtures";
import {
  browserSessionCookies,
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

const ROWS = ["AGT-064"];

// Fake capture hardware: recording needs a real MediaRecorder stream, which
// headless Chromium only offers with these flags.
test.use({
  launchOptions: {
    args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"],
  },
});

test(
  specTitle(ROWS, "operator dictation switch hides the mic, refuses writes and abandons a live recording"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt64"));
    const { owner, ownerSession, workspaceSlug } = harness;
    const cookies = browserSessionCookies(ownerSession);

    await test.step("owner stores a provider key and a speech key", async () => {
      await serverAssistantConfigPut(ownerSession, DUMMY_PROVIDER);
      await serverAssistantSttPut(ownerSession, DUMMY_STT);
      expect((await serverAssistantSttGet(ownerSession)).has_api_key).toBe(true);
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantStubTranscribeText("dictation should not land");
      // This checkout's backend predates the switch, so a target leg has to
      // serve the flag on first: the new app reads an absent flag as off.
      if (driver.target === "web_new") await driver.settingsProfileStubDictationEnabled(true);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 60_000 }).toBe("Hold to dictate");
      expect(await driver.assistantMicVisible()).toBe(true);
    });

    await test.step("flag absent: this checkout renders the ordinary dictation UI", async () => {
      // The pre-switch backend sends no `enabled` at all; the oracle leg can
      // only prove that an absent flag does not hide anything on its build.
      if (driver.target === "web_new") return;
      expect(await driver.assistantMicVisible()).toBe(true);
    });

    await test.step("flag off hides the composer mic and every hint", async () => {
      if (driver.target !== "web_new") return;
      await driver.settingsProfileStubDictationEnabled(false);
      const before = await driver.assistantSpeechConfigReads();
      await driver.assistantRefocus();
      await expect.poll(() => driver.assistantSpeechConfigReads(), { timeout: 30_000 }).toBeGreaterThan(before);
      await expect.poll(() => driver.assistantMicVisible(), { timeout: 30_000 }).toBe(false);
      expect(await driver.assistantMicLabel()).toBeNull();
      expect(await driver.assistantDictationHint()).toBeNull();
    });

    await test.step("the read still reports the flag while it is off", async () => {
      if (driver.target !== "web_new") return;
      const probe = await driver.assistantDictationApiProbe();
      expect(probe.read).toBe(200);
      expect(probe.readEnabled).toBe(false);
    });

    await test.step("a live recording is abandoned when the flag flips mid-recording", async () => {
      if (driver.target !== "web_new") return;
      await driver.settingsProfileStubDictationEnabled(true);
      await driver.assistantRefocus();
      await expect.poll(() => driver.assistantMicVisible(), { timeout: 30_000 }).toBe(true);
      await driver.assistantMicDown();
      await expect
        .poll(() => driver.assistantDictationHint(), { timeout: 60_000 })
        .toMatch(/Recording — release to transcribe/);
      await driver.settingsProfileStubDictationEnabled(false);
      await driver.assistantRefocus();
      await expect.poll(() => driver.assistantMicVisible(), { timeout: 30_000 }).toBe(false);
      await driver.assistantMicUp();
      // Nothing may upload after the switch turned the cell off.
      expect(await driver.assistantTranscribeRequests()).toEqual([]);
      expect(await driver.assistantDraftValue()).toBe("");
    });

    await test.step("save, test and transcribe answer not-found while delete stays open", async () => {
      if (driver.target !== "web_new") return;
      await driver.settingsProfileStubDictationWritesDisabled();
      const probe = await driver.assistantDictationApiProbe();
      expect(probe.read).toBe(200);
      expect(probe.readEnabled).toBe(false);
      expect(probe.save).toBe(404);
      expect(probe.test).toBe(404);
      expect(probe.transcribe).toBe(404);
      expect(probe.remove).toBe(204);
    });

    await test.step("voice settings narrow to remove-only while a key is saved", async () => {
      if (driver.target !== "web_new") return;
      await serverAssistantSttPut(ownerSession, DUMMY_STT);
      await driver.settingsProfileStubDictationEnabled(false);
      await driver.settingsProfileOpenTab("ai-assistant", cookies);
      await expect
        .poll(() => driver.settingsProfileDictationFacts(), { timeout: 30_000 })
        .toMatchObject({
          sectionVisible: true,
          removeVisible: true,
          saveDisabled: true,
          testDisabled: true,
          baseUrl: "",
          keyValue: "",
        });
    });

    await test.step("voice settings disappear once no key is saved", async () => {
      if (driver.target !== "web_new") return;
      await serverAssistantSttDelete(ownerSession);
      await driver.settingsProfileOpenTab("ai-assistant", cookies);
      await expect
        .poll(() => driver.settingsProfileDictationFacts(), { timeout: 30_000 })
        .toMatchObject({ sectionVisible: false, removeVisible: false });
      expect((await serverAssistantSttGet(ownerSession)).has_api_key).toBe(false);
    });

    await test.step("the saved key still offers dictation while the flag is on", async () => {
      await serverAssistantSttPut(ownerSession, DUMMY_STT);
      if (driver.target === "web_new") await driver.settingsProfileStubDictationEnabled(true);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 60_000 }).toBe("Hold to dictate");
      await driver.settingsProfileClearDictationStubs();
      await serverAssistantConfigDelete(ownerSession);
      await serverAssistantSttDelete(ownerSession);
    });
  }
);
