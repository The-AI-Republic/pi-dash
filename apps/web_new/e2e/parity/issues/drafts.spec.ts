// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: draft work items (NEWFRONT-120). Rows ISS-137–ISS-141.
// Drafts are workspace-scoped: scenarios create uniquely named drafts
// through the UI, assert what the user sees plus the server state, and
// remove only their own drafts afterwards. The shared seeded stack is
// never reset by these scenarios. The drafts page loads its blocks
// asynchronously, so every scenario first waits for it to settle into
// either the empty state or at least one rendered block.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  deleteServerDraft,
  deleteServerIssue,
  serverDraftNames,
  serverDrafts,
  serverIssueNames,
  serverIssues,
  signInSession,
} from "../helpers/api";
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

/**
 * Create the named draft through the UI unless a seed-linked one already
 * shows. The draft is always pointed at the seed project: the modal
 * pre-selects whatever project sorts first on the shared stack, which
 * would make project-linked assertions (identifier, default state)
 * nondeterministic — so a reused draft carrying a foreign project link
 * (left by an interrupted run) is recreated instead of kept.
 */
async function ensureDraft(driver: ParityDriver, seed: ParitySeedFacts, name: string): Promise<void> {
  await settleDrafts(driver, seed.workspaceSlug);
  if ((await driver.visibleDraftNames()).includes(name)) {
    if ((await driver.draftBlockText(name)).includes("PAR")) return;
    const session = await signInSession(seed.email, seed.password);
    const stale = (await serverDrafts(seed.workspaceSlug, session)).find((draft) => draft.name === name);
    if (stale) await deleteServerDraft(seed.workspaceSlug, stale.id, session);
    await driver.openDraftsPage(seed.workspaceSlug);
    await settleDrafts(driver, seed.workspaceSlug);
  }
  await driver.openCreateDraftModal();
  if ((await driver.modalProjectName()) !== seed.projectName) {
    await driver.selectModalProject(seed.projectName);
  }
  await driver.fillCreateTitle(name);
  await driver.submitCreateModal();
  await expect.poll(() => driver.visibleDraftNames(), { timeout: 120_000 }).toContain(name);
}

/**
 * Remove every draft this area owns (NF120 prefix), so menu-entry tests act
 * on a single block: each block owns its own menu, and each menu renders
 * the same untranslated entry keys, so a stray sibling block makes the
 * entry locator ambiguous. Other areas' drafts are never touched.
 */
async function clearOwnDrafts(workspaceSlug: string, session: string): Promise<void> {
  const drafts = await serverDrafts(workspaceSlug, session);
  for (const draft of drafts.filter((entry) => entry.name.startsWith("NF120 "))) {
    await deleteServerDraft(workspaceSlug, draft.id, session);
  }
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
      await ensureDraft(driver, seed, DRAFT_A);
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
  specTitle(["ISS-139"], "edit a draft through its quick action"),
  { tag: specTags(["ISS-139"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await signInAndOpenDrafts(driver, seed);
    });

    await test.step("open the drafts page with a known draft", async () => {
      await ensureDraft(driver, seed, DRAFT_B);
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
  specTitle(["ISS-139"], "copy a draft duplicates its payload"),
  { tag: specTags(["ISS-139"]) },
  async ({ driver, seed }) => {
    const NAME = `NF120 copy me ${Date.now()}`;
    const COPY = `${NAME} (copy)`;
    await test.step("sign in through the UI", async () => {
      await signInAndOpenDrafts(driver, seed);
    });

    await test.step("start from a single draft", async () => {
      const session = await signInSession(seed.email, seed.password);
      await clearOwnDrafts(seed.workspaceSlug, session);
      await driver.openDraftsPage(seed.workspaceSlug);
    });

    await test.step("open the drafts page with a known draft", async () => {
      await ensureDraft(driver, seed, NAME);
    });

    // Block counts are read with the modal closed: the open modal renders
    // its own matching divs, which would pollute the count.
    await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    const blocksBefore = await driver.draftBlockCount();

    await test.step("copying opens the duplicated payload", async () => {
      await driver.copyDraftByName(NAME);
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(true);
      expect(await driver.createModalHeading()).toContain("draft");
      expect(await driver.createTitleValue()).toBe(COPY);
    });

    await test.step("saving stores the duplicate next to the original", async () => {
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.visibleDraftNames(), { timeout: 120_000 }).toContain(COPY);
      expect(await driver.draftBlockCount()).toBe(blocksBefore + 1);
    });

    await test.step("the server holds both drafts", async () => {
      const session = await signInSession(seed.email, seed.password);
      await expect.poll(() => serverDraftNames(seed.workspaceSlug, session), { timeout: 60_000 }).toContain(COPY);
      const server = await serverDraftNames(seed.workspaceSlug, session);
      expect(server).toContain(NAME);
    });

    await test.step("cleanup removes only this run's drafts", async () => {
      const session = await signInSession(seed.email, seed.password);
      const drafts = await serverDrafts(seed.workspaceSlug, session);
      for (const title of [NAME, COPY]) {
        const mine = drafts.find((draft) => draft.name === title);
        expect(mine).toBeDefined();
        await deleteServerDraft(seed.workspaceSlug, mine!.id, session);
      }
      await expect.poll(() => serverDraftNames(seed.workspaceSlug, session), { timeout: 60_000 }).not.toContain(COPY);
    });
  }
);

test(
  specTitle(["ISS-139"], "move a draft to the project promotes it"),
  { tag: specTags(["ISS-139"]) },
  async ({ driver, seed }) => {
    const NAME = `NF120 promote me ${Date.now()}`;
    await test.step("sign in through the UI", async () => {
      await signInAndOpenDrafts(driver, seed);
    });

    await test.step("start from a single draft", async () => {
      const session = await signInSession(seed.email, seed.password);
      await clearOwnDrafts(seed.workspaceSlug, session);
      await driver.openDraftsPage(seed.workspaceSlug);
    });

    await test.step("create the draft against the seeded project", async () => {
      // The draft modal pre-selects whatever project sorts first on the
      // shared stack (usually a sibling run's); the move modal inherits
      // that project and its chip is read-only there, so point the draft
      // at the seed project up front and the promoted issue lands where
      // the assertions look for it.
      await settleDrafts(driver, seed.workspaceSlug);
      await driver.openCreateDraftModal();
      if ((await driver.modalProjectName()) !== seed.projectName) {
        await driver.selectModalProject(seed.projectName);
      }
      expect(await driver.modalProjectName()).toBe(seed.projectName);
      await driver.fillCreateTitle(NAME);
      await driver.submitCreateModal();
      await expect.poll(() => driver.visibleDraftNames(), { timeout: 120_000 }).toContain(NAME);
    });

    // Block counts are read with the modal closed: the open modal renders
    // its own matching divs, which would pollute the count.
    await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    const blocksBefore = await driver.draftBlockCount();

    await test.step("moving opens the move modal for the draft", async () => {
      await driver.moveDraftToProject(NAME);
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(true);
      expect(await driver.createTitleValue()).toBe(NAME);
      expect(await driver.modalProjectName()).toBe(seed.projectName);
      expect(await driver.modalTextContains("Add to project")).toBe(true);
    });

    await test.step("confirming promotes the draft into an issue", async () => {
      const session = await signInSession(seed.email, seed.password);
      await driver.confirmMoveToProject();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      await expect.poll(() => driver.pageTextContains("Draft published to project."), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.visibleDraftNames(), { timeout: 120_000 }).not.toContain(NAME);
      // The drafts count adjusts down by exactly this draft; the page does
      // not live-update, so no sibling run can disturb this count.
      expect(await driver.draftBlockCount()).toBe(blocksBefore - 1);
      await expect.poll(() => serverDraftNames(seed.workspaceSlug, session), { timeout: 60_000 }).not.toContain(NAME);
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .toContain(NAME);
    });

    await test.step("cleanup removes only the promoted issue", async () => {
      const session = await signInSession(seed.email, seed.password);
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
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
      await ensureDraft(driver, seed, DRAFT_A);
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
