// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): the chat surface offers no thread
// rename, archive or delete control on a populated landing or thread,
// while the PATCH/DELETE methods stay live server-side for a future
// surface. Row: AGT-058 (negative row).
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantThreadCreate,
  serverAssistantThreadDelete,
  serverAssistantThreadPatch,
  serverAssistantThreads,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, expectSidebarTitles } from "../assistant-core/support";

const ROWS = ["AGT-058"];

test(
  specTitle(ROWS, "no rename, archive or delete controls in chat; methods stay live"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt58"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    await test.step("owner prepares two titled threads", async () => {
      const first = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantThreadPatch(workspaceSlug, first.id, harness.ownerSession, { title: `keep-a-${tag}` });
      const second = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantThreadPatch(workspaceSlug, second.id, harness.ownerSession, { title: `keep-b-${tag}` });
      expect((await serverAssistantThreads(workspaceSlug, harness.ownerSession)).map((row) => row.title)).toEqual([
        `keep-b-${tag}`,
        `keep-a-${tag}`,
      ]);
    });

    await test.step("owner signs in and opens the landing", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenLanding(workspaceSlug);
      await expectSidebarTitles(driver, [`keep-b-${tag}`, `keep-a-${tag}`]);
    });

    await test.step("landing offers no thread management controls", async () => {
      expect(await driver.assistantSidebarButtons()).toEqual([]);
      expect(await driver.assistantThreadManagementControls()).toEqual([]);
    });

    await test.step("thread view offers no thread management controls", async () => {
      const threads = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, threads[0].id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
      await expectSidebarTitles(driver, [`keep-b-${tag}`, `keep-a-${tag}`]);
      expect(await driver.assistantSidebarButtons()).toEqual([]);
      expect(await driver.assistantThreadManagementControls()).toEqual([]);
    });

    await test.step("rename, archive and delete stay live server-side", async () => {
      const scratch = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const renamed = await serverAssistantThreadPatch(workspaceSlug, scratch.id, harness.ownerSession, {
        title: `scratch-${tag}`,
      });
      expect(renamed.title).toBe(`scratch-${tag}`);
      await serverAssistantThreadPatch(workspaceSlug, scratch.id, harness.ownerSession, { is_archived: true });
      expect(
        (await serverAssistantThreads(workspaceSlug, harness.ownerSession)).some((row) => row.id === scratch.id)
      ).toBe(false);
      await serverAssistantThreadPatch(workspaceSlug, scratch.id, harness.ownerSession, { is_archived: false });
      expect(
        (await serverAssistantThreads(workspaceSlug, harness.ownerSession)).some((row) => row.id === scratch.id)
      ).toBe(true);
      await serverAssistantThreadDelete(workspaceSlug, scratch.id, harness.ownerSession);
      expect(
        (await serverAssistantThreads(workspaceSlug, harness.ownerSession)).some((row) => row.id === scratch.id)
      ).toBe(false);
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
