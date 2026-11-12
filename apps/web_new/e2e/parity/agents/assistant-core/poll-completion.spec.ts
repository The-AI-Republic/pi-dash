// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): with the event stream down, a turn
// still completes through transcript polling — the composer unlocks and
// the error row renders once the poll observes the terminal row. (The
// poll path clears busy without setting the event-driven error line by
// design; the line only ever comes from a turn_failed event.)
// Skipped-server notices never count as terminal: a notice present
// mid-turn holds the composer busy until a real terminal row lands. Row: AGT-056.
import { test, expect } from "../../fixtures";
import type { AssistantStreamFrame } from "../../drivers/parity-driver";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantMessages,
  serverAssistantPollTerminal,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, PROVIDER_UNREACHABLE, expectBubbles, expectNoticeLines } from "./support";

const ROWS = ["AGT-056"];

test(
  specTitle(ROWS, "turns complete through the poll when the stream drops"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt56"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    await test.step("owner signs in", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
    });

    await test.step("a streamless turn completes through the poll", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantBlockStream(thread.id);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
      await driver.assistantFillDraft(`poll probe ${tag}`);
      await driver.assistantPressEnter();
      await expect.poll(() => driver.assistantStopVisible(), { timeout: 10_000 }).toBe(true);
      // No streamed row can arrive (every stream request aborts), so the
      // unlock below proves the poll path.
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 2);
      expect(bubbles).toEqual([
        { role: "user", text: `poll probe ${tag}` },
        { role: "error", text: PROVIDER_UNREACHABLE },
      ]);
      expect(await driver.assistantErrorLine()).toBeNull();
      await expect.poll(() => driver.assistantStopVisible(), { timeout: 30_000 }).toBe(false);
      expect(await driver.assistantSendVisible()).toBe(true);
      expect((await driver.assistantStreamRequestUrls(thread.id)).length).toBeGreaterThanOrEqual(1);
      await driver.assistantClearStreamBlock(thread.id);
      const messages = await serverAssistantMessages(workspaceSlug, thread.id, harness.ownerSession);
      expect(messages.map((row) => row.role)).toEqual(["user", "error"]);
    });

    await test.step("notices never count as terminal rows", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      // The stub stands in for the downed stream and carries only a
      // notice: the turn still completes through the poll, and only when
      // the error row — never the notice — lands.
      const notice: AssistantStreamFrame = {
        seq: 701,
        kind: "tool_servers_skipped",
        payload: { servers: [{ name: `poll-notice-${tag}`, reason: "toolset_unavailable" }] },
      };
      await driver.assistantStubStream(thread.id, [notice]);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      // The stubbed notice renders on connect ahead of any message row,
      // proving the stream is up before the send.
      expect(await expectNoticeLines(driver, (lines) => lines.length === 1)).toEqual([
        `Tool server poll-notice-${tag} was unavailable for this reply (it could not be reached).`,
      ]);
      await driver.assistantFillDraft(`notice probe ${tag}`);
      await driver.assistantPressEnter();
      let sawNoticeWhileBusy = false;
      await expect
        .poll(
          async () => {
            const lines = await driver.assistantNoticeLines();
            if (lines.length > 0 && (await driver.assistantStopVisible())) {
              sawNoticeWhileBusy = true;
            }
            return (await driver.assistantBubbles()).length;
          },
          { timeout: 90_000 }
        )
        .toBe(2);
      expect(sawNoticeWhileBusy).toBe(true);
      expect(await driver.assistantErrorLine()).toBeNull();
      await expect.poll(() => driver.assistantStopVisible(), { timeout: 30_000 }).toBe(false);
      await driver.assistantClearStreamStub(thread.id);
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
