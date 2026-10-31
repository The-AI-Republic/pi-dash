// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): the conversation sidebar deep-links
// every thread newest-first with an untitled fallback, highlights the
// open thread, hides archived threads, shows a placeholder when empty,
// and prepends a just-started thread without a reload. Row: AGT-044.
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
import { DUMMY_PROVIDER, UNTITLED, expectBubbles, expectSidebarTitles } from "./support";

const ROWS = ["AGT-044"];

test(
  specTitle(ROWS, "sidebar lists, links, highlights and prepends threads"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt44"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    await test.step("owner signs in and opens the landing", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenLanding(workspaceSlug);
    });

    await test.step("empty sidebar shows the placeholder", async () => {
      await expect.poll(() => driver.assistantSidebarEmptyVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.assistantSidebarThreads()).toEqual([]);
      expect(await serverAssistantThreads(workspaceSlug, harness.ownerSession)).toEqual([]);
    });

    const first = await test.step("owner prepares threads", async () => {
      const one = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantThreadPatch(workspaceSlug, one.id, harness.ownerSession, { title: `Alpha ${tag}` });
      const two = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantThreadPatch(workspaceSlug, two.id, harness.ownerSession, { title: `Beta ${tag}` });
      const untitled = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const archived = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantThreadPatch(workspaceSlug, archived.id, harness.ownerSession, {
        title: `Gone ${tag}`,
        is_archived: true,
      });
      return { one, two, untitled, archived };
    });

    await test.step("threads list newest-first with links and fallback", async () => {
      await driver.assistantOpenLanding(workspaceSlug);
      await expectSidebarTitles(driver, [UNTITLED, `Beta ${tag}`, `Alpha ${tag}`]);
      const rows = await driver.assistantSidebarThreads();
      expect(rows.map((row) => row.href)).toEqual([
        `/${workspaceSlug}/assistant/${first.untitled.id}`,
        `/${workspaceSlug}/assistant/${first.two.id}`,
        `/${workspaceSlug}/assistant/${first.one.id}`,
      ]);
      expect(rows.every((row) => row.active === false)).toBe(true);
      const listed = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      expect(listed.map((row) => row.id)).toEqual([first.untitled.id, first.two.id, first.one.id]);
    });

    await test.step("the open thread highlights and loads on click", async () => {
      await driver.assistantClickSidebarThread(1);
      await expect
        .poll(() => driver.assistantCurrentPath(), { timeout: 60_000 })
        .toBe(`/${workspaceSlug}/assistant/${first.two.id}`);
      const rows = await driver.assistantSidebarThreads();
      expect(rows.map((row) => row.active)).toEqual([false, true, false]);
      // Beta has no messages yet: the generic empty prompt shows.
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
    });

    await test.step("a just-started thread prepends without a reload", async () => {
      await driver.assistantClickNewChat();
      await expect.poll(() => driver.assistantCurrentPath(), { timeout: 60_000 }).toBe(`/${workspaceSlug}/assistant`);
      // The path updates before the new route renders: wait for the
      // landing composer itself, or the fill below lands in the stale
      // thread composer that shares its placeholder.
      await expect.poll(() => driver.assistantLandingComposerVisible(), { timeout: 60_000 }).toBe(true);
      // The first send is held back so the optimistic entry — untitled,
      // ahead of any revalidation — is observable before the title lands.
      await driver.assistantDelaySend(5000);
      const draft = `sidebar prepend ${tag}`;
      await driver.assistantFillDraft(draft);
      // The fill stuck (DOM value) and React picked it up (send enabled
      // reads component state): the submit below cannot misfire on a
      // desynced draft.
      expect(await driver.assistantDraftValue()).toBe(draft);
      expect(await driver.assistantSendEnabled()).toBe(true);
      await driver.assistantPressEnter();
      let path = "";
      await expect
        .poll(
          async () => {
            path = await driver.assistantCurrentPath();
            return path !== `/${workspaceSlug}/assistant`;
          },
          { timeout: 60_000 }
        )
        .toBe(true);
      const startedId = path.split("/").pop() ?? "";
      await expectSidebarTitles(driver, [UNTITLED, UNTITLED, `Beta ${tag}`, `Alpha ${tag}`]);
      await driver.assistantClearSendDelay();
      await expectSidebarTitles(driver, [draft, UNTITLED, `Beta ${tag}`, `Alpha ${tag}`]);
      const rows = await driver.assistantSidebarThreads();
      expect(rows[0]?.href).toBe(`/${workspaceSlug}/assistant/${startedId}`);
      expect(rows[0]?.active).toBe(true);
      await expectBubbles(driver, (bubbles) => bubbles.some((row) => row.text === draft));
      await serverAssistantPollTerminal(workspaceSlug, startedId, harness.ownerSession);
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
