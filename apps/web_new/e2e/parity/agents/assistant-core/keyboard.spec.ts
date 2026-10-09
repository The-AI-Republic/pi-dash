// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): Enter submits from the multiline thread
// composer and from the single-line dashboard input, Shift+Enter inserts a
// newline, and blank thread-composer drafts never send. (No window-level key
// handlers exist on this surface — the only key handlers sit on the
// composer textarea, the widget input and the mic button itself — so
// there are no global hotkeys by construction.) bug: NEWFRONT-195 —
// modified Enter (Control+Enter) also submits; intended is unmodified
// Enter only. Row: AGT-049.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantMessages,
  serverAssistantPollTerminal,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, PROVIDER_UNREACHABLE, expectBubbles } from "./support";

const ROWS = ["AGT-049"];

test(
  specTitle(ROWS, "bug: NEWFRONT-195 modified Enter submits; plain Enter, newline and blank hold"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt49"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
    });

    await test.step("Shift+Enter inserts a newline without sending", async () => {
      await driver.assistantStartApiSpy();
      await driver.assistantFillDraft("first line");
      await driver.assistantPressShiftEnter();
      expect(await driver.assistantDraftValue()).toBe("first line\n");
      expect((await driver.assistantApiCounts()).send).toBe(0);
      expect(await serverAssistantMessages(workspaceSlug, thread.id, harness.ownerSession)).toEqual([]);
    });

    const sent = `second line ${tag}`;
    await test.step("Enter submits the multiline draft", async () => {
      await driver.assistantFillDraft(`first line\n${sent}`);
      await driver.assistantPressEnter();
      expect(await driver.assistantDraftValue()).toBe("");
      const bubbles = await expectBubbles(driver, (rows) =>
        rows.some((row) => row.role === "user" && row.text === `first line\n${sent}`)
      );
      expect(bubbles[0]).toEqual({ role: "user", text: `first line\n${sent}` });
      expect((await driver.assistantApiCounts()).send).toBe(1);
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
    });

    await test.step("blank drafts never send", async () => {
      await driver.assistantFillDraft("   ");
      await driver.assistantPressEnter();
      await driver.assistantFillDraft("");
      await driver.assistantPressEnter();
      expect((await driver.assistantApiCounts()).send).toBe(1);
      const messages = await serverAssistantMessages(workspaceSlug, thread.id, harness.ownerSession);
      expect(messages.filter((row) => row.role === "user").length).toBe(1);
    });

    await test.step("bug: NEWFRONT-195 modified Enter submits", async () => {
      await driver.assistantFillDraft(`modified ${tag}`);
      await driver.assistantPressControlEnter();
      // The full shape: the first turn's error row can trail the second
      // user row through a slow refetch, so the poll waits for all three.
      const bubbles = await expectBubbles(
        driver,
        (rows) => rows.length === 3 && rows.filter((row) => row.role === "user").length === 2
      );
      expect(bubbles).toEqual([
        { role: "user", text: `first line\nsecond line ${tag}` },
        { role: "error", text: PROVIDER_UNREACHABLE },
        { role: "user", text: `modified ${tag}` },
      ]);
      expect((await driver.assistantApiCounts()).send).toBe(2);
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
      await driver.assistantStopApiSpy();
    });

    await test.step("Enter submits from the single-line widget input", async () => {
      await driver.assistantOpenHome(workspaceSlug);
      await expect.poll(() => driver.assistantCardVisible(), { timeout: 60_000 }).toBe(true);
      const draft = `widget enter ${tag}`;
      await driver.assistantCardFillDraft(draft);
      await driver.assistantCardPressEnter();
      let path = "";
      await expect
        .poll(
          async () => {
            path = await driver.assistantCurrentPath();
            return path !== `/${workspaceSlug}`;
          },
          { timeout: 60_000 }
        )
        .toBe(true);
      const widgetThread = path.split("/").pop() ?? "";
      const messages = await serverAssistantPollTerminal(workspaceSlug, widgetThread, harness.ownerSession);
      expect(messages.filter((row) => row.role === "user").map((row) => row.content)).toEqual([draft]);
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
