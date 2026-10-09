// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): the assistant landing greets a keyed
// member with a headline, a caption and a standalone composer; blank
// submits never leave the page; a failed start surfaces a notice and
// creates nothing; a real submit locks the composer while starting,
// creates exactly one untitled thread and navigates into it carrying the
// first message. Row: AGT-038.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantPollTerminal,
  serverAssistantThreads,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, assistantToast } from "./support";

const ROWS = ["AGT-038"];

test(
  specTitle(ROWS, "landing greets, guards blanks and starts exactly one thread per submit"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt38"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      const config = await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
      expect(config.has_api_key).toBe(true);
    });

    await test.step("owner signs in and opens the landing", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenLanding(workspaceSlug);
    });

    await test.step("greeting, caption and composer show, no setup card", async () => {
      const greeting = await driver.assistantLandingGreeting();
      expect(greeting).toEqual({
        headline: "How can I help you today?",
        caption: "Ask about your issues, create work, or start a coding run.",
      });
      expect(await driver.assistantLandingComposerVisible()).toBe(true);
      expect(await driver.assistantSetupCard()).toBeNull();
    });

    await test.step("blank submits never leave the page", async () => {
      await driver.assistantStartApiSpy();
      await driver.assistantFillDraft("   ");
      expect(await driver.assistantSendEnabled()).toBe(false);
      await driver.assistantPressEnter();
      expect(await driver.assistantCurrentPath()).toBe(`/${workspaceSlug}/assistant`);
      expect((await driver.assistantApiCounts()).threadCreate).toBe(0);
    });

    await test.step("a failed start toasts and creates nothing", async () => {
      await driver.assistantFailThreadCreateOnce();
      await driver.assistantFillDraft(`will fail ${tag}`);
      await driver.assistantClickSend();
      const message = await assistantToast(driver, "Unable to start chat");
      expect(message).toContain("parity thread-create failure");
      expect(await driver.assistantCurrentPath()).toBe(`/${workspaceSlug}/assistant`);
      expect(await serverAssistantThreads(workspaceSlug, harness.ownerSession)).toEqual([]);
      await driver.assistantClearThreadCreateStubs();
    });

    const firstMessage = `hello assistant ${tag}`;
    const threadId = await test.step("submit locks, creates once and navigates", async () => {
      await driver.assistantDelayThreadCreate(2500);
      await driver.assistantFillDraft(firstMessage);
      await driver.assistantClickSend();
      // The starting window locks the inputs, so a second submit cannot
      // fire while the create is in flight.
      await expect.poll(() => driver.assistantTextareaDisabled(), { timeout: 10_000 }).toBe(true);
      let path = "";
      await expect
        .poll(
          async () => {
            path = await driver.assistantCurrentPath();
            return path.startsWith(`/${workspaceSlug}/assistant/`) && path !== `/${workspaceSlug}/assistant`;
          },
          { timeout: 60_000 }
        )
        .toBe(true);
      await driver.assistantClearThreadCreateStubs();
      // Only the successful create falls through to the spy (the stubbed
      // failure fulfills without falling through): exactly one start.
      expect((await driver.assistantApiCounts()).threadCreate).toBe(1);
      await driver.assistantStopApiSpy();
      return path.split("/").pop() ?? "";
    });

    await test.step("server state carries the thread and its first message", async () => {
      expect(threadId).not.toBe("");
      const threads = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      expect(threads.map((thread) => thread.id)).toEqual([threadId]);
      const messages = await serverAssistantPollTerminal(workspaceSlug, threadId, harness.ownerSession);
      const userRows = messages.filter((row) => row.role === "user");
      expect(userRows.map((row) => row.content)).toEqual([firstMessage]);
      const titled = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      expect(titled[0]?.title).toBe(firstMessage);
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
