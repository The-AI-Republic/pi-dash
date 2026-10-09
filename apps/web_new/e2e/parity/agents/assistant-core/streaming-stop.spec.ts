// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): a send swaps the composer to stop while
// the turn runs, with the transcript polling behind the stream; stopping
// fires cancel and stays locked until the terminal state lands, and turn
// failures render as error rows with the provider detail. bug:
// NEWFRONT-197 — live SSE frames are corrupt server-side so the client
// drops every live event: completion here runs through the poll (the
// error line, set only from the turn_failed event, never appears).
// Intended is stop-then-server-event-confirm with the inline line. Token
// deltas and reconnect dedupe are proven at the transport contract with
// stubbed SSE frames (the seeded stack has no model backend): the
// running text appends in arrival order, a replayed duplicate never
// double-appends, and a reconnect replay settles on the same transcript.
// Row: AGT-042.
import { test, expect } from "../../fixtures";
import type { AssistantStreamFrame } from "../../drivers/parity-driver";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantMessages,
  serverAssistantThreadCreate,
  serverAssistantThreads,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, PROVIDER_UNREACHABLE, expectBubbles } from "./support";

const ROWS = ["AGT-042"];

function stubMessage(id: string, content: string, status: string): Record<string, unknown> {
  return {
    id,
    role: "assistant",
    content,
    status,
    seq: 1,
    turn_id: "stub-turn",
    payload: {},
    created_at: "2026-01-01T00:00:00Z",
    completed_at: null,
  };
}

test(
  specTitle(ROWS, "bug: NEWFRONT-197 live events drop, stop-then-poll-confirm holds; stubbed deltas dedupe"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt42"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
    });

    await test.step("send swaps to stop and polls behind the stream", async () => {
      await driver.assistantStartApiSpy();
      await driver.assistantFillDraft(`stream probe ${tag}`);
      await driver.assistantClickSend();
      await expect.poll(() => driver.assistantStopVisible(), { timeout: 10_000 }).toBe(true);
      expect(await driver.assistantSendVisible()).toBe(false);
    });

    await test.step("stop fires cancel and stays locked until the terminal lands", async () => {
      await driver.assistantClickStop();
      await expect.poll(async () => (await driver.assistantApiCounts()).cancel, { timeout: 30_000 }).toBe(1);
      // Still locked right after the stop; the composer unlocks only when
      // the terminal failure lands.
      expect(await driver.assistantStopVisible()).toBe(true);
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 2);
      expect(bubbles).toEqual([
        { role: "user", text: `stream probe ${tag}` },
        { role: "error", text: PROVIDER_UNREACHABLE },
      ]);
      // bug NEWFRONT-197: the turn_failed event is dropped with every
      // other live event, so the event-driven error line never appears —
      // the poll observes the error row and unlocks instead.
      expect(await driver.assistantErrorLine()).toBeNull();
      expect(await driver.assistantStopVisible()).toBe(false);
      expect(await driver.assistantSendVisible()).toBe(true);
      const counts = await driver.assistantApiCounts();
      expect(counts.send).toBe(1);
      expect(counts.messageList).toBeGreaterThanOrEqual(2);
      await driver.assistantStopApiSpy();
    });

    await test.step("server state shows the failed turn", async () => {
      const messages = await serverAssistantMessages(workspaceSlug, thread.id, harness.ownerSession);
      expect(messages.map((row) => [row.role, row.status])).toEqual([
        ["user", "completed"],
        ["error", "failed"],
      ]);
      expect(messages[1]?.content).toBe(PROVIDER_UNREACHABLE);
      const threads = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      expect(threads[0]?.has_active_turn).toBe(false);
    });

    await test.step("stubbed deltas append in arrival order without doubling", async () => {
      const stubbed = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const created: AssistantStreamFrame = {
        seq: 101,
        kind: "message_created",
        message: "stub-m1",
        payload: { turn_id: "stub-turn", message: stubMessage("stub-m1", "", "streaming") },
      };
      const deltaOne: AssistantStreamFrame = {
        seq: 102,
        kind: "assistant_delta",
        message: "stub-m1",
        payload: { turn_id: "stub-turn", params: { delta: "Hello " } },
      };
      // Same event seq served twice: the replay dedupe must skip it.
      const deltaOneDup: AssistantStreamFrame = { ...deltaOne };
      const deltaTwo: AssistantStreamFrame = {
        seq: 103,
        kind: "assistant_delta",
        message: "stub-m1",
        payload: { turn_id: "stub-turn", params: { delta: "world" } },
      };
      await driver.assistantStubStream(stubbed.id, [created, deltaOne, deltaOneDup, deltaTwo]);
      await driver.assistantOpenThread(workspaceSlug, stubbed.id);
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 1 && rows[0]?.text === "Hello world");
      expect(bubbles).toEqual([{ role: "assistant", text: "Hello world" }]);
      const completed: AssistantStreamFrame = {
        seq: 104,
        kind: "message_completed",
        message: "stub-m1",
        payload: {
          turn_id: "stub-turn",
          message: stubMessage("stub-m1", "Hello world", "completed"),
        },
      };
      const done: AssistantStreamFrame = {
        seq: 105,
        kind: "turn_completed",
        payload: { turn_id: "stub-turn", usage: {} },
      };
      // Re-served without `created` (the row exists by now): replays skip
      // the duplicate deltas and re-apply the completion, so the text is
      // stable across reconnects instead of flapping through a reset.
      await driver.assistantStubStream(stubbed.id, [deltaOne, deltaOneDup, deltaTwo, completed, done]);
      // The stream's natural reconnect replays the sequence; the
      // transcript settles on the same single bubble either way.
      await expectBubbles(driver, (rows) => rows.length === 1 && rows[0]?.text === "Hello world");
      await expect
        .poll(async () => (await driver.assistantStreamRequestUrls(stubbed.id)).length, { timeout: 60_000 })
        .toBeGreaterThanOrEqual(2);
      await driver.assistantClearStreamStub(stubbed.id);
      expect(await driver.assistantBubbles()).toEqual([{ role: "assistant", text: "Hello world" }]);
      const urls = await driver.assistantStreamRequestUrls(stubbed.id);
      expect(urls.length).toBeGreaterThanOrEqual(2);
      for (const url of urls) {
        // The client replays from zero with a bare URL (no after cursor).
        expect(new URL(url).search).toBe("");
      }
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
