// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): the drafts non-behaviors — no bulk,
// drag, shortcut, search or refetch affordances with stale-until-reload
// reads — and the cross-edition smoke pin. Rows: DRAFT-026, DRAFT-027.
// Green on apps/web first.
import { test, expect } from "../fixtures";
import { serverCreateDraft, serverDraftRecord, serverPatchDraftStatus } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs } from "./support";

test(
  specTitle(["DRAFT-026"], "the screen offers no bulk, drag, shortcut, search or refetch affordances"),
  { tag: specTags(["DRAFT-026"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d26");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D26 Row ${harness.tag}`;
    await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("no bulk, drag, search, filter or export controls", async () => {
      await draftsOpenAs(driver, harness);
      expect(await driver.draftsMainCheckboxCount()).toBe(0);
      expect(await driver.draftRowsDraggableCount()).toBe(0);
      const text = await driver.draftsMainText();
      expect(text).not.toMatch(/filter/i);
      expect(text).not.toMatch(/export/i);
      expect(text).not.toMatch(/search/i);
    });

    await test.step("shortcut-like keys change nothing", async () => {
      const before = await driver.currentPath();
      for (const key of ["e", "/", "?", "c"]) {
        await driver.draftsPressKey(key);
      }
      expect(await driver.createModalOpen()).toBe(false);
      expect(await driver.currentPath()).toBe(before);
      expect(await driver.draftRowNames()).toContain(name);
    });

    await test.step("refocusing fetches nothing", async () => {
      expect(await driver.draftsListFetchCountOnRefocus()).toBe(0);
    });

    await test.step("outside changes appear only after a fresh fetch", async () => {
      const renamed = `D26 Renamed ${harness.tag}`;
      const draftId = await draftIdOf(workspaceSlug, owner.cookie, name);
      expect((await driver.draftRowNames())[0]).toBe(name);
      await serverPatchDraftStatus(workspaceSlug, draftId, owner.cookie, { name: renamed });
      expect((await serverDraftRecord(workspaceSlug, draftId, owner.cookie)).name).toBe(renamed);
      expect(await driver.draftRowNames()).toContain(name);
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(renamed);
    });
  }
);

async function draftIdOf(workspaceSlug: string, cookie: string, name: string): Promise<string> {
  const { serverDraftsPage } = await import("../helpers/api");
  const page = await serverDraftsPage(workspaceSlug, cookie, "");
  const found = page.drafts.find((draft) => draft.name === name);
  if (!found) throw new Error(`[parity] draft ${JSON.stringify(name)} missing from the server list.`);
  return found.id;
}

test(
  specTitle(["DRAFT-027"], "drafts behave the same across editions"),
  { tag: specTags(["DRAFT-027"]) },
  async ({ driver }) => {
    // Drafts ship no edition-specific surface: the same screen, driven
    // over the same endpoints, renders on every build. The scenario pins
    // the standard surface and the read-only entry traffic it runs on.
    const harness = await draftsHarness("parity-d27");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D27 Row ${harness.tag}`;
    await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("the standard surface renders", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
      expect(await driver.draftsPageTitle()).toBe("Workspace Draft");
      expect(await driver.draftsHeaderText()).toContain("Drafts");
    });

    await test.step("entering drafts fires only read-only draft calls", async () => {
      const calls = await driver.draftsEntryRequests(workspaceSlug);
      expect(calls.length).toBeGreaterThan(0);
      for (const call of calls) {
        expect(call.startsWith("GET ")).toBe(true);
        expect(call).toContain("/draft-issues");
      }
    });
  }
);
