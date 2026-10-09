// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): blank drafts never send (the button
// disables and Enter is a no-op; the API refuses blanks and overlong
// bodies); a send shows a posting state, then clears the draft and
// refreshes thread ordering; failures — a busy-turn conflict or the
// provider failing — render inline without losing the transcript. bug:
// NEWFRONT-197 — live SSE frames are corrupt server-side so the client
// drops every live event: the provider failure lands as an error row
// through the poll (the event-driven line never appears) and a stale
// conflict line is never cleared by the dropped terminal event.
// Intended is the terminal line with the provider detail. Row: AGT-043.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantMessages,
  serverAssistantPollTerminal,
  serverAssistantSendRaw,
  serverAssistantThreadCreate,
  serverAssistantThreads,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, PROVIDER_UNREACHABLE, expectBubbles, expectSidebarTitles } from "./support";

const ROWS = ["AGT-043"];

test(
  specTitle(ROWS, "bug: NEWFRONT-197 failures land as rows, conflict line goes stale; blank/posting hold"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt43"));
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

    await test.step("blank drafts are ignored", async () => {
      await driver.assistantStartApiSpy();
      await driver.assistantFillDraft("   ");
      expect(await driver.assistantSendEnabled()).toBe(false);
      await driver.assistantPressEnter();
      expect(await driver.assistantDraftValue()).toBe("   ");
      expect((await driver.assistantApiCounts()).send).toBe(0);
      expect(await serverAssistantMessages(workspaceSlug, thread.id, harness.ownerSession)).toEqual([]);
    });

    await test.step("the API refuses blank and overlong bodies", async () => {
      const blank = await serverAssistantSendRaw(workspaceSlug, thread.id, "   ", harness.ownerSession);
      expect(blank.status).toBe(400);
      expect((blank.payload as { error?: unknown }).error).toBe("empty_message");
      const long = await serverAssistantSendRaw(workspaceSlug, thread.id, "x".repeat(32001), harness.ownerSession);
      expect(long.status).toBe(400);
      expect((long.payload as { error?: unknown }).error).toBe("message_too_long");
    });

    const sent = `posting probe ${tag}`;
    await test.step("send posts, clears and refreshes ordering", async () => {
      await driver.assistantDelaySend(2500);
      await driver.assistantFillDraft(sent);
      await driver.assistantPressEnter();
      // Posting locks the textarea until the send resolves.
      await expect.poll(() => driver.assistantTextareaDisabled(), { timeout: 10_000 }).toBe(true);
      await driver.assistantClearSendDelay();
      expect(await driver.assistantDraftValue()).toBe("");
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 2);
      expect(bubbles).toEqual([
        { role: "user", text: sent },
        { role: "error", text: PROVIDER_UNREACHABLE },
      ]);
      // bug NEWFRONT-197: the dropped turn_failed event never sets the
      // event-driven error line — the failure lands as a row only.
      expect(await driver.assistantErrorLine()).toBeNull();
      // The send refreshed the sidebar without a reload.
      await expectSidebarTitles(driver, [sent]);
      expect((await driver.assistantApiCounts()).send).toBe(1);
    });

    await test.step("a busy-turn conflict lands inline without losing rows", async () => {
      const busy = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, busy.id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
      await driver.assistantFillDraft(`first ${tag}`);
      await driver.assistantPressEnter();
      await driver.assistantFillDraft(`second ${tag}`);
      await driver.assistantPressEnter();
      // The second send raced the running turn: the raw conflict code
      // shows inline from the send response.
      await expect.poll(() => driver.assistantErrorLine(), { timeout: 10_000 }).toBe("turn_active");
      await serverAssistantPollTerminal(workspaceSlug, busy.id, harness.ownerSession);
      // bug NEWFRONT-197: the dropped turn_failed event never replaces
      // the stale conflict line, so it persists past the terminal state.
      expect(await driver.assistantErrorLine()).toBe("turn_active");
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 2);
      expect(bubbles).toEqual([
        { role: "user", text: `first ${tag}` },
        { role: "error", text: PROVIDER_UNREACHABLE },
      ]);
      const messages = await serverAssistantMessages(workspaceSlug, busy.id, harness.ownerSession);
      expect(messages.map((row) => row.role)).toEqual(["user", "error"]);
      expect(messages[0]?.content).toBe(`first ${tag}`);
      await driver.assistantStopApiSpy();
    });

    await test.step("thread ordering follows sends", async () => {
      const threads = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      expect(threads.map((row) => row.title)).toEqual([`first ${tag}`, sent]);
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
