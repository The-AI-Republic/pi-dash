// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): the server titles a new thread from the
// first message's first line, and the sidebar picks the title up after
// the send without a manual reload; untitled threads keep the fallback
// label meanwhile. (The generate-title helper serves only the
// create-issue modal — no chat surface calls it.) Row: AGT-054.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantPollTerminal,
  serverAssistantThreadCreate,
  serverAssistantThreads,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, UNTITLED, expectSidebarTitles } from "./support";

const ROWS = ["AGT-054"];

test(
  specTitle(ROWS, "first send titles the thread and the sidebar updates live"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt54"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    const thread = await test.step("owner prepares an untitled thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
    });

    await test.step("sidebar shows the untitled fallback", async () => {
      await expectSidebarTitles(driver, [UNTITLED]);
    });

    const firstLine = `a title from ${tag}`;
    await test.step("first send titles from the first line only", async () => {
      await driver.assistantFillDraft(`${firstLine}\nsecond line stays out of the title`);
      await driver.assistantPressEnter();
      await expectSidebarTitles(driver, [firstLine]);
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
    });

    await test.step("server state carries the server-derived title", async () => {
      const threads = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      expect(threads.map((row) => row.title)).toEqual([firstLine]);
    });

    await test.step("long first lines truncate to sixty characters", async () => {
      const long = `long ${tag} ${"x".repeat(80)}`;
      const roomy = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, roomy.id);
      await driver.assistantFillDraft(long);
      await driver.assistantPressEnter();
      await expectSidebarTitles(driver, [long.slice(0, 60), firstLine]);
      await serverAssistantPollTerminal(workspaceSlug, roomy.id, harness.ownerSession);
      const threads = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      expect(threads[0]?.title).toBe(long.slice(0, 60));
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
