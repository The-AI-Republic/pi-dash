// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): opening a draft for editing by
// double-click or row-menu entry, and the row menu's actions each
// performing their own outcome. Rows: DRAFT-008, DRAFT-009. Green on
// apps/web first.
//
// DRAFT-009's translated-labels half is a bug: scenario (the oracle
// renders raw i18n keys); that scenario lives in this file once its
// linked issue exists.
import { test, expect } from "../fixtures";
import { serverCreateDraft, serverDraftNames, serverDraftRecord } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs } from "./support";

test(
  specTitle(["DRAFT-008"], "double-click opens the draft prefilled; saving updates the row in place"),
  { tag: specTags(["DRAFT-008"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d8");
    const { owner, workspaceSlug, projectId, projectName } = harness;
    const name = `D8 Edit ${harness.tag}`;
    const renamed = `D8 Renamed ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("double-click opens the dialog prefilled", async () => {
      await draftsOpenAs(driver, harness);
      await driver.openDraftForEdit(name);
      expect(await driver.createTitleValue()).toBe(name);
      expect(await driver.modalProjectName()).toBe(projectName);
    });

    await test.step("saving updates the row in place", async () => {
      await driver.fillCreateTitle(renamed);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(renamed);
      expect(await driver.draftRowNames()).not.toContain(name);
      expect((await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).name).toBe(renamed);
    });
  }
);

test(
  specTitle(["DRAFT-008"], "the row-menu edit entry opens the draft prefilled and saves in place"),
  { tag: specTags(["DRAFT-008"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d8b");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D8 Menu ${harness.tag}`;
    const renamed = `D8 Menu Renamed ${harness.tag}`;
    const draft = await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("the menu edit entry opens the dialog prefilled", async () => {
      await draftsOpenAs(driver, harness);
      await driver.editDraftByName(name);
      expect(await driver.createTitleValue()).toBe(name);
    });

    await test.step("saving updates the row in place", async () => {
      await driver.fillCreateTitle(renamed);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(renamed);
      expect((await serverDraftRecord(workspaceSlug, draft.id, owner.cookie)).name).toBe(renamed);
    });
  }
);

test(
  specTitle(["DRAFT-009"], "row menu and quick action expose entries that each perform their outcome"),
  { tag: specTags(["DRAFT-009"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d9");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D9 Actions ${harness.tag}`;
    await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("the hover control exposes the row menu", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftRowQuickActionVisible(name), { timeout: 60_000 }).toBe(true);
      const entries = await driver.draftRowMenuEntries(name);
      expect(entries).toHaveLength(4);
    });

    await test.step("duplicate opens the carried payload", async () => {
      await driver.draftRowMenuClick(name, "make_a_copy");
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(true);
      expect(await driver.createTitleValue()).toBe(`${name} (copy)`);
      // Cleanup only (DRAFT-005 owns the close prompt): the prefilled copy
      // trips the dialog's dirty check only when the payload lands after
      // mount, so Discard may prompt or close silently depending on that
      // race; clear whichever close path appears.
      await driver.clickModalDiscard();
      await expect
        .poll(
          async () => {
            if (await driver.pageTextContains("Save this draft?")) {
              await driver.discardDialogDiscard();
              return "prompted";
            }
            return (await driver.createModalOpen()) ? "open" : "closed";
          },
          { timeout: 30_000 }
        )
        .not.toBe("open");
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      expect(await serverDraftNames(workspaceSlug, owner.cookie)).not.toContain(`${name} (copy)`);
    });

    await test.step("delete opens the confirmation", async () => {
      await driver.cancelDraftDelete(name);
      expect(await serverDraftNames(workspaceSlug, owner.cookie)).toContain(name);
    });

    await test.step("move opens the move-to-project modal", async () => {
      await driver.draftRowMenuClick(name, "move_to_project");
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(true);
      expect(await driver.modalTextContains("Add to project")).toBe(true);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      expect(await serverDraftNames(workspaceSlug, owner.cookie)).toContain(name);
    });
  }
);

// The oracle renders the row-menu titles untranslated; DRAFT-009 (as
// amended to the main#532 target) requires translated labels. Pinned
// here until NEWFRONT-298 lands the fix.
test(
  specTitle(["DRAFT-009"], "bug: row menus render raw i18n keys (NEWFRONT-298)"),
  { tag: specTags(["DRAFT-009"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d9bug");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `D9 Keys ${harness.tag}`;
    await serverCreateDraft(workspaceSlug, owner.cookie, { name, project_id: projectId });

    await test.step("both openers expose the raw keys in order", async () => {
      await draftsOpenAs(driver, harness);
      expect(await driver.draftRowMenuEntries(name)).toEqual(["edit", "make_a_copy", "move_to_project", "delete"]);
      expect(await driver.draftContextMenuEntries(name)).toEqual(["edit", "make_a_copy", "move_to_project", "delete"]);
    });
  }
);
