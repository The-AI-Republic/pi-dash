// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): push-to-talk dictation records with a
// live timer, transcribes on release, and appends with spacing fixed;
// sub-half-second taps discard without uploading. The transcription
// itself runs at the transport contract (stubbed; the seeded stack has
// no speech backend), disclosed here. (The flag-off halves stay a Gap in
// the row: VOICE_DICTATION_ENABLED ships in PR #586, still open, and
// this checkout renders the mic unflagged.) Row: AGT-050.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantSttDelete,
  serverAssistantSttGet,
  serverAssistantSttPut,
  serverAssistantThreadCreate,
  serverAssistantTranscribeRaw,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, DUMMY_STT } from "../assistant-core/support";

const ROWS = ["AGT-050"];

// Fake capture hardware: the record path needs a real MediaRecorder
// stream, which headless Chromium only offers with these flags.
test.use({
  launchOptions: {
    args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"],
  },
});

test(
  specTitle(ROWS, "hold records with a timer, release transcribes into the draft"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt50"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores dummy provider and speech keys", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
      await serverAssistantSttPut(harness.ownerSession, DUMMY_STT);
      expect((await serverAssistantSttGet(harness.ownerSession)).has_api_key).toBe(true);
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantStubTranscribeText(`dictated ${tag}`);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 60_000 }).toBe("Hold to dictate");
    });

    await test.step("hold records with a live timer, release transcribes", async () => {
      await driver.assistantMicDown();
      await expect
        .poll(() => driver.assistantDictationHint(), { timeout: 60_000 })
        .toMatch(/Recording — release to transcribe/);
      expect(await driver.assistantMicLabel()).toBe("Release to transcribe");
      await driver.assistantMicUpAfter(2000);
      await expect.poll(() => driver.assistantDraftValue(), { timeout: 60_000 }).toBe(`dictated ${tag}`);
      const uploads = await driver.assistantTranscribeRequests();
      expect(uploads.length).toBe(1);
      expect(uploads[0].contentType).toContain("multipart/form-data");
      expect(uploads[0].hasFilePart).toBe(true);
      expect(uploads[0].byteLength).toBeGreaterThan(0);
    });

    await test.step("transcripts append with spacing fixed", async () => {
      await driver.assistantStubTranscribeText("world");
      await driver.assistantFillDraft("hello");
      await driver.assistantMicHold(2500);
      await expect.poll(() => driver.assistantDraftValue(), { timeout: 60_000 }).toBe("hello world");
      expect((await driver.assistantTranscribeRequests()).length).toBe(2);
    });

    await test.step("sub-half-second taps discard without uploading", async () => {
      await driver.assistantMicHold(100);
      await expect.poll(() => driver.assistantMicLabel(), { timeout: 30_000 }).toBe("Hold to dictate");
      expect((await driver.assistantTranscribeRequests()).length).toBe(2);
      expect(await driver.assistantDictationHint()).toBeNull();
    });

    await test.step("transcription failures explain in the hint", async () => {
      await driver.assistantFailTranscribe(500, { error: "transcribe_failed", detail: `parity boom ${tag}` });
      await driver.assistantMicHold(2500);
      await expect.poll(() => driver.assistantDictationHint(), { timeout: 60_000 }).toBe(`parity boom ${tag}`);
      await driver.assistantClearTranscribeStubs();
    });

    await test.step("server gates refuse keyless and fileless posts", async () => {
      await serverAssistantSttDelete(harness.ownerSession);
      const keyless = await serverAssistantTranscribeRaw(harness.ownerSession, {
        bytes: new Uint8Array([1, 2, 3]),
        filename: "clip.webm",
        contentType: "audio/webm",
      });
      expect(keyless.status).toBe(422);
      expect((keyless.payload as { error?: unknown }).error).toBe("stt_config_missing");
      await serverAssistantSttPut(harness.ownerSession, DUMMY_STT);
      const fileless = await serverAssistantTranscribeRaw(harness.ownerSession, null);
      expect(fileless.status).toBe(400);
      expect((fileless.payload as { error?: unknown }).error).toBe("no_audio");
    });

    await test.step("both keys are restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
      await serverAssistantSttDelete(harness.ownerSession);
    });
  }
);
