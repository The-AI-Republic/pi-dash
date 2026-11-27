// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): the assistant history is the assistant's
// own link list — a New-chat link entry plus link thread rows, with no
// buttons — and never the shared button-based history panel, which stays
// reserved for runner chat. Row: AGT-061 (structural negative row).
// NEWPIDASH-337: the new app opens that same link list from a Chat History
// button in the page header (no persistent rail), while the oracle keeps
// its persistent rail. The driver abstracts the DOM difference (header
// label, row and button reads), so this scenario runs unchanged against
// both.
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
  specTitle(ROWS, "assistant history is its own link list, never the shared panel"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt61"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    await test.step("owner prepares two titled threads", async () => {
      const first = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantThreadPatch(workspaceSlug, first.id, harness.ownerSession, { title: `history-a-${tag}` });
      const second = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantThreadPatch(workspaceSlug, second.id, harness.ownerSession, { title: `history-b-${tag}` });
    });

    await test.step("owner signs in and opens the landing", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenLanding(workspaceSlug);
      await expectSidebarTitles(driver, [`history-b-${tag}`, `history-a-${tag}`]);
    });

    await test.step("landing history is link-based with its own label", async () => {
      // The label is the rail brand on the oracle and the history button on
      // the new app; either way it is the assistant's own surface, never
      // the shared panel's "Chats" heading.
      const header = await driver.assistantSidebarHeader();
      expect(header).not.toBeNull();
      expect(header).not.toBe("Chats");
      expect(await driver.assistantSidebarRowKinds()).toEqual({ newChat: "a", rows: ["a", "a"] });
      expect(await driver.assistantSidebarButtons()).toEqual([]);
    });

    await test.step("thread history keeps the same shape", async () => {
      const threads = await serverAssistantThreads(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, threads[0].id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
      const header = await driver.assistantSidebarHeader();
      expect(header).not.toBeNull();
      expect(header).not.toBe("Chats");
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
