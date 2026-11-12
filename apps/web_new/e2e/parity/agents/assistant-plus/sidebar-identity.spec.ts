// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): the assistant sidebar is the layout's
// own link-based rail — branded header, link New-chat entry, link thread
// rows — and never the shared button-based history panel, which stays
// reserved for runner chat (its only consumer besides its unit test).
// Row: AGT-061 (structural negative row).
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantThreadCreate,
  serverAssistantThreadPatch,
  serverAssistantThreads,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, expectSidebarTitles } from "../assistant-core/support";

const ROWS = ["AGT-061"];

test(
  specTitle(ROWS, "assistant sidebar is its own link rail, never the shared panel"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt61"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    await test.step("owner prepares two titled threads", async () => {
      const first = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantThreadPatch(workspaceSlug, first.id, harness.ownerSession, { title: `rail-a-${tag}` });
      const second = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantThreadPatch(workspaceSlug, second.id, harness.ownerSession, { title: `rail-b-${tag}` });
    });

    await test.step("owner signs in and opens the landing", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenLanding(workspaceSlug);
      await expectSidebarTitles(driver, [`rail-b-${tag}`, `rail-a-${tag}`]);
    });

    await test.step("landing sidebar is link-based with its own header", async () => {
      expect(await driver.assistantSidebarHeader()).toBe("Pi Dash AI");
      expect(await driver.assistantSidebarRowKinds()).toEqual({ newChat: "a", rows: ["a", "a"] });
      expect(await driver.assistantSidebarButtons()).toEqual([]);
    });

    await test.step("thread sidebar keeps the same shape", async () => {
      const threads = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, threads[0].id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
      expect(await driver.assistantSidebarHeader()).toBe("Pi Dash AI");
      expect(await driver.assistantSidebarRowKinds()).toEqual({ newChat: "a", rows: ["a", "a"] });
      const sidebar = await driver.assistantSidebarThreads();
      expect(sidebar.map((row) => row.title)).toEqual(
        (await serverAssistantThreads(workspaceSlug, harness.ownerSession)).map((row) => row.title)
      );
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
