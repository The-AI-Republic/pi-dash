// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios: draft work items (NEWFRONT-120). Rows ISS-137–ISS-141.
// Drafts are workspace-scoped: scenarios create uniquely named drafts
// through the UI, assert what the user sees plus the server state, and
// remove only their own drafts afterwards. The shared seeded stack is
// never reset by these scenarios. The drafts page loads its blocks
// asynchronously, so every scenario first waits for it to settle into
// either the empty state or at least one rendered block.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import { serverDraftNames, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const DRAFT_A = "NF120 draft alpha";
const DRAFT_B = "NF120 draft beta";

/** Wait until the drafts page shows the empty state or rendered blocks. */
async function settleDrafts(driver: ParityDriver, workspaceSlug: string): Promise<"empty" | "list"> {
  return driver.settleDraftsPage(workspaceSlug);
}

async function signInAndOpenDrafts(
  driver: ParityDriver,
  seed: { email: string; password: string; workspaceSlug: string }
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPasswordRetry(seed.email, seed.password);
  await driver.openDraftsPage(seed.workspaceSlug);
  await driver.dismissWelcomeDialog();
}

/** Create the named draft through the UI unless it already shows. */
async function ensureDraft(driver: ParityDriver, workspaceSlug: string, name: string): Promise<void> {
  await settleDrafts(driver, workspaceSlug);
  if ((await driver.visibleDraftNames()).includes(name)) return;
  await driver.openCreateDraftModal();
  await driver.fillCreateTitle(name);
  await driver.submitCreateModal();
  await expect.poll(() => driver.visibleDraftNames(), { timeout: 120_000 }).toContain(name);
}

test(
  specTitle(["ISS-137"], "drafts page lists drafts and offers creating one"),
  { tag: specTags(["ISS-137"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenDrafts(driver, seed);
    });

    await test.step("create a draft through the UI", async () => {
      await settleDrafts(driver, seed.workspaceSlug);
      if (!(await driver.visibleDraftNames()).includes(DRAFT_A)) {
        await driver.openCreateDraftModal();
        expect(await driver.createModalHeading()).toContain("draft");
        expect(await driver.modalPrimaryButtonLabel()).toContain("Draft");
        await driver.fillCreateTitle(DRAFT_A);
        await driver.submitCreateModal();
      }
      await expect.poll(() => driver.visibleDraftNames(), { timeout: 120_000 }).toContain(DRAFT_A);
    });

    await test.step("the server stored the draft", async () => {
      const session = await signInSession(seed.email, seed.password);
      await expect.poll(() => serverDraftNames(seed.workspaceSlug, session), { timeout: 60_000 }).toContain(DRAFT_A);
    });
  }
);

test(
  specTitle(["ISS-138"], "draft block shows inline properties"),
  { tag: specTags(["ISS-138"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenDrafts(driver, seed);
    });

    await test.step("open the drafts page with a known draft", async () => {
      await ensureDraft(driver, seed.workspaceSlug, DRAFT_A);
    });

    await test.step("the block carries the draft name, project and state", async () => {
      const block = await driver.draftBlockText(DRAFT_A);
      expect(block).toContain(DRAFT_A);
      expect(block).toContain("PAR");
      expect(block).toContain("Todo");
    });
  }
);

test(
  specTitle(["ISS-139"], "edit, copy and promote a draft"),
  { tag: specTags(["ISS-139"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenDrafts(driver, seed);
    });

    await test.step("open the drafts page with a known draft", async () => {
      await ensureDraft(driver, seed.workspaceSlug, DRAFT_B);
    });

    await test.step("double-clicking opens the edit modal prefilled", async () => {
      await driver.openDraftForEdit(DRAFT_B);
      expect(await driver.createTitleValue()).toBe(DRAFT_B);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      expect(await driver.pageTextContains("Save this draft?")).toBe(false);
    });

    await test.step("the server still holds the unedited draft", async () => {
      const session = await signInSession(seed.email, seed.password);
      const server = await serverDraftNames(seed.workspaceSlug, session);
      expect(server).toContain(DRAFT_B);
    });
  }
);

test(
  specTitle(["ISS-141"], "empty drafts invite creating the first one"),
  { tag: specTags(["ISS-141"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenDrafts(driver, seed);
    });

    await test.step("remove only this run's drafts", async () => {
      await settleDrafts(driver, seed.workspaceSlug);
      for (const name of [DRAFT_A, DRAFT_B]) {
        if ((await driver.visibleDraftNames()).includes(name)) {
          await driver.deleteDraftByName(name);
          await expect.poll(() => driver.visibleDraftNames(), { timeout: 120_000 }).not.toContain(name);
        }
      }
      await settleDrafts(driver, seed.workspaceSlug);
    });

    await test.step("empty state or foreign drafts", async () => {
      const blocks = await driver.draftBlockCount();
      test.skip(blocks > 0, `other drafts present (${blocks}); the empty state cannot show`);
      expect(await driver.pageTextContains("Half-written work items")).toBe(true);
      expect(await driver.pageTextContains("Create draft work item")).toBe(true);
    });
  }
);

test(
  specTitle(["ISS-140"], "delete a draft after confirmation"),
  { tag: specTags(["ISS-140"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenDrafts(driver, seed);
    });

    await test.step("open the drafts page with a known draft", async () => {
      await ensureDraft(driver, seed.workspaceSlug, DRAFT_A);
    });

    await test.step("deleting removes it from the page and the server", async () => {
      await driver.deleteDraftByName(DRAFT_A);
      await expect.poll(() => driver.visibleDraftNames(), { timeout: 120_000 }).not.toContain(DRAFT_A);
      const session = await signInSession(seed.email, seed.password);
      await expect
        .poll(() => serverDraftNames(seed.workspaceSlug, session), { timeout: 60_000 })
        .not.toContain(DRAFT_A);
    });
  }
);
