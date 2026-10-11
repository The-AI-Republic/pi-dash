// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-32): draft creation — the header action's
// role gate, the save flow, and blank-title defaulting. Rows: DRAFT-003,
// DRAFT-004, DRAFT-006. Green on apps/web first.
import { test, expect } from "../fixtures";
import { ROLE, serverCreateDraft, serverDraftNames } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { draftsHarness, draftsOpenAs, draftsSeat } from "./support";

test(
  specTitle(["DRAFT-003"], "header creation is gated to project members with admin or member roles"),
  { tag: specTags(["DRAFT-003"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d3");

    await test.step("an admin with a project gets an enabled control that opens the dialog", async () => {
      await draftsOpenAs(driver, harness);
      await expect.poll(() => driver.draftsHeaderCreateState(), { timeout: 60_000 }).toBe("enabled");
      await driver.openCreateDraftModal();
      expect(await driver.createModalOpen()).toBe(true);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("a member with a project gets the enabled control", async () => {
      const member = await draftsSeat(harness, ROLE.MEMBER, "parity-d3-member", ROLE.MEMBER);
      await draftsOpenAs(driver, harness, member);
      await expect.poll(() => driver.draftsHeaderCreateState(), { timeout: 60_000 }).toBe("enabled");
    });

    await test.step("a guest with a project sees the control disabled", async () => {
      const guest = await draftsSeat(harness, ROLE.GUEST, "parity-d3-guest", ROLE.GUEST);
      await draftsOpenAs(driver, harness, guest);
      await expect.poll(() => driver.draftsHeaderCreateState(), { timeout: 60_000 }).toBe("disabled");
    });

    await test.step("a project-less member gets no working action", async () => {
      const outsider = await draftsSeat(harness, ROLE.MEMBER, "parity-d3-outsider");
      await draftsOpenAs(driver, harness, outsider);
      await expect.poll(() => driver.draftsHeaderCreateState(), { timeout: 60_000 }).toBe("absent");
    });
  }
);

test(
  specTitle(["DRAFT-004"], "saving a new draft tops the list with a success notice and resets the dialog"),
  { tag: specTags(["DRAFT-004"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d4");
    const { owner, workspaceSlug, projectId, projectName } = harness;
    const name = `D4 Created ${harness.tag}`;

    await test.step("an older draft sorts below the new one", async () => {
      await serverCreateDraft(workspaceSlug, owner.cookie, { name: `D4 Older ${harness.tag}`, project_id: projectId });
      await draftsOpenAs(driver, harness);
    });

    await test.step("fill in and save through the creation dialog", async () => {
      await driver.openCreateDraftModal();
      if ((await driver.modalProjectName()) !== projectName) {
        await driver.selectModalProject(projectName);
      }
      expect(await driver.modalProjectName()).toBe(projectName);
      await driver.fillCreateTitle(name);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("the saved draft tops the list with a success notice", async () => {
      await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toContain("Draft created");
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain(name);
      expect((await driver.draftRowNames())[0]).toBe(name);
      await expect.poll(() => serverDraftNames(workspaceSlug, owner.cookie), { timeout: 60_000 }).toContain(name);
    });

    await test.step("the dialog closes and resets", async () => {
      await driver.openCreateDraftModal();
      expect(await driver.createTitleValue()).toBe("");
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });
  }
);

// The creation-dialog submit half of DRAFT-006 is a bug: scenario (the
// oracle blocks a blank title with "Title is required" instead of
// defaulting). Pinned here until NEWFRONT-302 lands the fix.
test(
  specTitle(["DRAFT-006"], "bug: blank-title submit fails validation instead of defaulting (NEWFRONT-302)"),
  { tag: specTags(["DRAFT-006"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d6bug");
    const { workspaceSlug, projectName } = harness;

    for (const title of ["", "   "]) {
      await test.step(`submitting ${JSON.stringify(title)} keeps the dialog open with an error`, async () => {
        await draftsOpenAs(driver, harness);
        await driver.openCreateDraftModal();
        if ((await driver.modalProjectName()) !== projectName) {
          await driver.selectModalProject(projectName);
        }
        if (title.length > 0) {
          await driver.fillCreateTitle(title);
        }
        await driver.submitCreateModal();
        await expect.poll(() => driver.createTitleError(), { timeout: 30_000 }).toBe("Title is required");
        expect(await driver.createModalOpen()).toBe(true);
        await driver.clickModalDiscard();
        if (await driver.pageTextContains("Save this draft?")) {
          await driver.discardDialogDiscard();
        }
        await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
        expect(await driver.draftRowNames()).not.toContain("Untitled");
      });
    }
    expect(await serverDraftNames(workspaceSlug, harness.owner.cookie)).not.toContain("Untitled");
  }
);

test(
  specTitle(["DRAFT-006"], "keep-as-draft with a blank title stores an Untitled draft"),
  { tag: specTags(["DRAFT-006"]) },
  async ({ driver }) => {
    const harness = await draftsHarness("parity-d6");
    const { workspaceSlug, projectName } = harness;

    await test.step("keep content with no title as a draft", async () => {
      await draftsOpenAs(driver, harness);
      await driver.openCreateDraftModal();
      if ((await driver.modalProjectName()) !== projectName) {
        await driver.selectModalProject(projectName);
      }
      expect(await driver.createTitleValue()).toBe("");
      await driver.fillDescription(`Untitled body ${harness.tag}`);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.pageTextContains("Save this draft?"), { timeout: 30_000 }).toBe(true);
      await driver.confirmSaveDraft();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("the stored draft carries the placeholder title", async () => {
      await expect.poll(() => driver.draftRowNames(), { timeout: 60_000 }).toContain("Untitled");
      await expect
        .poll(() => serverDraftNames(workspaceSlug, harness.owner.cookie), { timeout: 60_000 })
        .toContain("Untitled");
    });
  }
);
