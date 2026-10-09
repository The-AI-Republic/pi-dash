// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): the dashboard assistant card offers
// quick entry — a single-line input with Ask, suggestion chips that fill
// the draft, and up to five recent threads deep-linking out — and blocks
// keyless sends with an error notice instead of starting. (Card presence
// itself is SHELL-007, already green; guests see nothing per AGT-041.)
// Row: AGT-048.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantPollTerminal,
  serverAssistantThreadCreate,
  serverAssistantThreadPatch,
  serverAssistantThreads,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { API_KEY_REMINDER, DUMMY_PROVIDER, assistantToast, dismissTour } from "./support";

const ROWS = ["AGT-048"];

test(
  specTitle(ROWS, "dashboard card suggests, lists recents and blocks keyless sends"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt48"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    const titles = await test.step("owner prepares six titled threads", async () => {
      const names = [1, 2, 3, 4, 5, 6].map((n) => `Recent ${tag} ${n}`);
      for (const name of names) {
        const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
        await serverAssistantThreadPatch(workspaceSlug, thread.id, harness.ownerSession, { title: name });
      }
      return names;
    });

    await test.step("owner signs in and opens the dashboard", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenHome(workspaceSlug);
      await dismissTour(driver);
    });

    await test.step("input, Ask and suggestion chips show", async () => {
      await expect.poll(() => driver.assistantCardVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.assistantCardDraftValue()).toBe("");
      expect(await driver.assistantCardAskDisabled()).toBe(true);
      expect(await driver.assistantCardSuggestions()).toEqual([
        "What's assigned to me?",
        "Create an issue in…",
        "Summarize open issues in…",
      ]);
    });

    await test.step("chips fill the draft without sending", async () => {
      await driver.assistantCardClickSuggestion("What's assigned to me?");
      expect(await driver.assistantCardDraftValue()).toBe("What's assigned to me?");
      expect(await driver.assistantCurrentPath()).toBe(`/${workspaceSlug}`);
      expect((await serverAssistantThreads(workspaceSlug, harness.ownerSession)).length).toBe(6);
      expect(await driver.assistantCardAskDisabled()).toBe(false);
    });

    await test.step("up to five recents deep-link out", async () => {
      let recents: { title: string; href: string }[] = [];
      await expect
        .poll(
          async () => {
            recents = await driver.assistantCardRecents();
            return recents.length;
          },
          { timeout: 60_000 }
        )
        .toBe(5);
      // Newest first; the sixth (oldest) falls off.
      expect(recents.map((recent) => recent.title)).toEqual([titles[5], titles[4], titles[3], titles[2], titles[1]]);
      for (const recent of recents) {
        expect(recent.href).toContain(`/assistant/`);
      }
      await driver.assistantCardClickRecent(0);
      const threads = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      await expect
        .poll(() => driver.assistantCurrentPath(), { timeout: 60_000 })
        .toBe(`/${workspaceSlug}/assistant/${threads[0]?.id}`);
    });

    await test.step("Ask starts a thread from the card", async () => {
      await driver.assistantOpenHome(workspaceSlug);
      await expect.poll(() => driver.assistantCardVisible(), { timeout: 60_000 }).toBe(true);
      const draft = `card hello ${tag}`;
      await driver.assistantCardFillDraft(draft);
      await driver.assistantCardClickAsk();
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
      const threadId = path.split("/").pop() ?? "";
      const messages = await serverAssistantPollTerminal(workspaceSlug, threadId, harness.ownerSession);
      expect(messages.filter((row) => row.role === "user").map((row) => row.content)).toEqual([draft]);
    });

    await test.step("keyless sends block with an error notice", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
      await driver.assistantOpenHome(workspaceSlug);
      await expect.poll(() => driver.assistantCardVisible(), { timeout: 60_000 }).toBe(true);
      // The keyless card still renders its entry; recents prove it.
      expect((await driver.assistantCardRecents()).length).toBeGreaterThan(0);
      await driver.assistantCardFillDraft(`blocked ${tag}`);
      await driver.assistantCardClickAsk();
      const message = await assistantToast(driver, "API key required");
      expect(message).toBe(API_KEY_REMINDER);
      expect(await driver.assistantCurrentPath()).toBe(`/${workspaceSlug}`);
      // Six prepared plus the one card-started thread; the blocked send
      // created nothing.
      expect((await serverAssistantThreads(workspaceSlug, harness.ownerSession)).length).toBe(7);
    });
  }
);
