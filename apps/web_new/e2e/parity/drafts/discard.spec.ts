// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): the new-item dialog's close guard —
// unsaved content offers keep-as-draft or discard, empty content closes
// silently. Row: DRAFT-005. Green on apps/web first.
import { test, expect } from "../fixtures";
import { serverDraftNames } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs } from "./support";

test(
  specTitle(["DRAFT-005"], "closing with content offers keep-as-draft or discard"),
  { tag: specTags(["DRAFT-005"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d5");
    const { owner, workspaceSlug, projectName } = harness;
    const kept = `D5 Kept ${harness.tag}`;
    const dropped = `D5 Dropped ${harness.tag}`;

    await test.step("keep-as-draft stores the content and closes everything", async () => {
      await draftsOpenAs(driver, harness);
      await driver.openCreateDraftModal();
      if ((await driver.modalProjectName()) !== projectName) {
        await driver.selectModalProject(projectName);
      }
      await driver.fillCreateTitle(kept);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.pageTextContains("Save this draft?"), { timeout: 30_000 }).toBe(true);
      await driver.confirmSaveDraft();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      expect(await driver.pageTextContains("Save this draft?")).toBe(false);
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(kept);
      await expect.poll(() => serverDraftNames(workspaceSlug, owner.cookie), { timeout: 60_000 }).toContain(kept);
    });

    await test.step("discard closes without storing", async () => {
      await driver.openCreateDraftModal();
      await driver.fillCreateTitle(dropped);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.pageTextContains("Save this draft?"), { timeout: 30_000 }).toBe(true);
      await driver.discardDialogDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      expect(await driver.pageTextContains("Save this draft?")).toBe(false);
      expect(await driver.draftRowNames()).not.toContain(dropped);
      expect(await serverDraftNames(workspaceSlug, owner.cookie)).not.toContain(dropped);
    });
  }
);

test(
  specTitle(["DRAFT-005"], "closing with no content never prompts"),
  { tag: specTags(["DRAFT-005"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d5b");

    await test.step("an untouched dialog closes silently", async () => {
      await draftsOpenAs(driver, harness);
      await driver.openCreateDraftModal();
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      expect(await driver.pageTextContains("Save this draft?")).toBe(false);
    });
  }
);
