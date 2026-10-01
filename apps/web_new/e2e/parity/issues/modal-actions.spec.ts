// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios: modal pickers and row actions (NEWFRONT-120). Rows
// ISS-122, ISS-126, ISS-132–ISS-136. Scenarios drive the row quick actions
// and the modal pickers through the UI, assert what the user sees plus the
// server state, and remove only their own issues, labels, and states
// afterwards. The archive flow needs a completed state, which no seed
// provides, so the scenario creates one through the API and deletes it
// again; the move flow runs against the single seeded project, so its
// oracle is the documented empty state.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  createServerState,
  deleteServerIssue,
  deleteServerState,
  patchServerIssue,
  serverCreateIssue,
  serverIssueRecord,
  serverIssueNames,
  serverIssues,
  serverProject,
  serverProjects,
  serverStates,
  signInSession,
  unarchiveServerIssue,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/**
 * Open a row menu entry and prove the modal belongs to `name`'s issue:
 * live list re-sorts on the shared stack can land the click on a
 * neighboring row, which would otherwise archive/delete/rename a
 * stranger. `verify` must identify the issue (modal text or title
 * value). Retries with Escape between attempts.
 */
async function openRowMenuForIssue(
  driver: ParityDriver,
  name: string,
  entry: string,
  verify: () => Promise<boolean>
): Promise<void> {
  for (let attempt = 0; attempt < 3; attempt++) {
    await driver.openRowMenuEntry(name, entry);
    const ok = await expect
      .poll(verify, { timeout: 15_000 })
      .toBe(true)
      .then(() => true)
      .catch(() => false);
    if (ok) return;
    await driver.pressKey("Escape");
  }
  await driver.openRowMenuEntry(name, entry);
  await expect.poll(verify, { timeout: 30_000 }).toBe(true);
}

async function signInAndOpenIssues(
  driver: {
    openEntry(): Promise<void>;
    signInWithPasswordRetry(e: string, p: string): Promise<void>;
    openProjectIssuesSettled(w: string, p: string): Promise<void>;
    dismissWelcomeDialog(): Promise<void>;
    visibleIssueNames(): Promise<string[]>;
  },
  seed: { email: string; password: string; workspaceSlug: string; projectId: string; issueNames: string[] }
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPasswordRetry(seed.email, seed.password);
  await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  await driver.dismissWelcomeDialog();
  // The shared stack stalls the list fetch under sibling load and the
  // list does not retry on its own, so reload a few times before giving
  // up instead of polling one dead page for two minutes.
  for (let attempt = 0; attempt < 3; attempt++) {
    const loaded = await expect
      .poll(() => driver.visibleIssueNames(), { timeout: 30_000 })
      .toEqual(expect.arrayContaining([...seed.issueNames]))
      .then(() => true)
      .catch(() => false);
    if (loaded) return;
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  }
  await expect
    .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
    .toEqual(expect.arrayContaining([...seed.issueNames]));
}

/** Create an issue through the modal and return when it is stored. */
async function createIssue(
  driver: {
    openCreateModal(): Promise<void>;
    fillCreateTitle(n: string): Promise<void>;
    submitCreateModal(): Promise<void>;
    createModalOpen(): Promise<boolean>;
    openProjectIssuesSettled(w: string, p: string): Promise<void>;
  },
  session: string,
  seed: { workspaceSlug: string; projectId: string },
  name: string
): Promise<void> {
  await driver.openCreateModal();
  await driver.fillCreateTitle(name);
  await driver.submitCreateModal();
  await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
  await expect
    .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
    .toContain(name);
  // Reload so later row actions see a freshly fetched list instead of the
  // pre-create render (the list does not always refetch on its own).
  await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
}

test(
  specTitle(["ISS-126"], "edit an issue through the modal"),
  { tag: specTags(["ISS-126"]) },
  async ({ driver, seed }) => {
    // Timestamped: concurrent shared-stack runs must never share a name.
    const NAME = `NF120 edit me ${Date.now()}`;
    const RENAMED = `${NAME} edited`;
    const DISCARDED = `${NAME} discarded`;
    await signInAndOpenIssues(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    await createIssue(driver, session, seed, NAME);

    await test.step("the edit modal diffs and updates", async () => {
      // The title field proves the modal opened for our row, not a
      // neighbor the live list shifted under the click.
      await openRowMenuForIssue(driver, NAME, "Edit", () => driver.createTitleValue().then((value) => value === NAME));
      expect(await driver.createModalHeading()).toContain("Update");
      expect(await driver.modalButtonDisabled(seed.projectName)).toBe(true);
      await driver.fillCreateTitle(RENAMED);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      await expect
        .poll(() => driver.pageTextContains("Work item updated successfully"), { timeout: 30_000 })
        .toBe(true);
    });

    await test.step("the server stored the rename", async () => {
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .toContain(RENAMED);
    });

    await test.step("discarding an edit never asks for a draft", async () => {
      await openRowMenuForIssue(driver, RENAMED, "Edit", () =>
        driver.createTitleValue().then((value) => value === RENAMED)
      );
      await driver.fillCreateTitle(DISCARDED);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      expect(await driver.pageTextContains("Save this draft?")).toBe(false);
      const names = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(names).toContain(RENAMED);
      expect(names).not.toContain(DISCARDED);
    });

    await test.step("cleanup removes only this run's issue", async () => {
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      // NAME is timestamped per run; match the rename and any un-renamed
      // leftover from a failed middle step.
      const mine = all.filter((issue) => issue.name.includes(NAME));
      expect(mine.length).toBeGreaterThan(0);
      for (const issue of mine) await deleteServerIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
    });
  }
);

test(
  specTitle(["ISS-122"], "attach, change and remove a parent in the modal"),
  { tag: specTags(["ISS-122"]) },
  async ({ driver, seed }) => {
    await signInAndOpenIssues(driver, seed);
    await driver.openCreateModal();

    await test.step("searching finds the seeded issue with a new-tab link", async () => {
      await driver.openParentPicker();
      await driver.searchParentInModal("Parity second");
      await expect.poll(() => driver.modalTextContains("Parity second issue"), { timeout: 30_000 }).toBe(true);
      expect(await driver.parentResultNewTabLinks()).toBeGreaterThan(0);
    });

    await test.step("selecting sets the parent tag", async () => {
      await driver.selectParentResult("Parity second issue");
      expect(await driver.modalTextContains("Parity second issue")).toBe(true);
    });

    await test.step("removing clears the tag", async () => {
      await driver.removeParentInModal("Parity second issue");
      expect(await driver.modalTextContains("Parity second issue")).toBe(false);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });
  }
);

test(
  specTitle(["ISS-132"], "label picker lists labels and creates one inline"),
  { tag: specTags(["ISS-132"]) },
  async ({ driver, seed }) => {
    const STAMP = Date.now();
    const LABEL = `nf120-parity-label-${STAMP}`;
    const NAME = `NF120 labelled ${STAMP}`;
    await signInAndOpenIssues(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    await driver.openCreateModal();

    await test.step("an admin creates the label inline and selects it", async () => {
      await driver.openLabelsPicker();
      await driver.createLabelInModal(LABEL);
      await expect.poll(() => driver.selectedLabelVisible(LABEL), { timeout: 30_000 }).toBe(true);
    });

    await test.step("saving stores the label on the issue", async () => {
      await driver.fillCreateTitle(NAME);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      const detail = await serverIssueRecord(seed.workspaceSlug, seed.projectId, mine!.id, session);
      expect(((detail["label_ids"] ?? []) as unknown[]).length).toBeGreaterThan(0);
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
    });
  }
);

test(
  specTitle(["ISS-133"], "hovering shows the work-item preview card"),
  { tag: specTags(["ISS-133"]) },
  async ({ driver, seed }) => {
    // The preview card opens on hover of dated calendar blocks
    // (openOnHover popover). Issues are created through the API: the card
    // (not the creation flow) is under test, and UI creation would not fit
    // the test budget on slow runs. The shared seed is never mutated.
    // Calendar placement keys off the target date: start-only issues
    // render no block anywhere, so the start-only date branch has no hover
    // surface (like the neither-set branch) and stays uncovered by design.
    const STAMP = Date.now();
    const RANGE = `NF120 preview range ${STAMP}`;
    const OVERDUE = `NF120 preview overdue ${STAMP}`;
    const NODATE = `NF120 preview nodate ${STAMP}`;
    const dayOffset = (days: number): string => new Date(Date.now() + days * 86400000).toISOString().slice(0, 10);
    // The card renders "MMM dd, yyyy" offset-free for date-only values.
    const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const cardDate = (iso: string): string =>
      `${MONTHS[Number(iso.slice(5, 7)) - 1]} ${iso.slice(8, 10)}, ${iso.slice(0, 4)}`;
    // Keep the plain range inside this month so it stays on the visible
    // calendar grid whatever day the suite runs (falling back to tomorrow
    // at month's end, when only grid spillover covers it).
    const thisMonth = dayOffset(0).slice(0, 7);
    const future = [7, 3, 1].map(dayOffset).find((iso) => iso.slice(0, 7) === thisMonth) ?? dayOffset(1);
    const yesterday = dayOffset(-1);
    await signInAndOpenIssues(driver, seed);
    const session = await signInSession(seed.email, seed.password);

    // The shared helper posts without retries; the scratch stack throttles
    // under sibling load, so retry creation a few times here.
    const createApiIssue = async (name: string): Promise<string> => {
      let last: unknown;
      for (let attempt = 1; attempt <= 4; attempt++) {
        try {
          return await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name);
        } catch (error) {
          last = error;
          await new Promise((resolve) => setTimeout(resolve, 10000 * attempt));
        }
      }
      throw last;
    };

    await test.step("setup: dated issues with distinct priorities", async () => {
      await createApiIssue(RANGE);
      await createApiIssue(OVERDUE);
      await createApiIssue(NODATE);
      const listed = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const idOf = (name: string): string => listed.find((issue) => issue.name === name)!.id;
      await patchServerIssue(seed.workspaceSlug, seed.projectId, idOf(RANGE), session, {
        start_date: future,
        target_date: future,
        priority: "urgent",
      });
      await patchServerIssue(seed.workspaceSlug, seed.projectId, idOf(OVERDUE), session, {
        target_date: yesterday,
      });
      await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
      await expect.poll(() => driver.countText(RANGE), { timeout: 120_000 }).toBeGreaterThan(0);
    });

    // Identifier, state and title the card must repeat, read back from the
    // server so the assertions pin the link, not a hardcoded guess.
    const listed = await serverIssues(seed.workspaceSlug, seed.projectId, session);
    const rangeId = listed.find((issue) => issue.name === RANGE)!.id;
    const rangeDetail = await serverIssueRecord(seed.workspaceSlug, seed.projectId, rangeId, session);
    const identifier = `${(await serverProject(seed.workspaceSlug, seed.projectId, session)).identifier}-${String(rangeDetail["sequence_id"])}`;
    const states = await serverStates(seed.workspaceSlug, seed.projectId, session);
    const stateName = states.find((state) => state.id === String(rangeDetail["state_id"]))!.name;

    await test.step("an undated row shows a tooltip, not a preview card", async () => {
      // Only calendar and gantt blocks carry the preview card, and neither
      // renders an issue with no dates at all — so the neither-set branch
      // has no hover surface. The list row hover pops a title tooltip
      // instead; pin that, so the null case stays honestly uncovered
      // rather than silently untested.
      const before = await driver.countText(NODATE);
      await driver.hoverIssueRow(NODATE);
      await expect.poll(() => driver.countText(NODATE), { timeout: 30_000 }).toBeGreaterThan(before);
      expect((await driver.hoverCardRead(NODATE)).text).toBe("");
    });

    await test.step("the calendar renders the dated issues", async () => {
      await driver.switchIssueLayout("Calendar Layout");
      await expect.poll(() => driver.countText(RANGE), { timeout: 120_000 }).toBeGreaterThan(0);
    });

    let plainColor = "";
    await test.step("the range card shows identifier, state, title, priority and dates", async () => {
      const card = await driver.hoverCardRead(RANGE);
      expect(card.text).toContain(identifier);
      expect(card.text).toContain(stateName);
      expect(card.text).toContain(RANGE);
      expect(card.text).toContain(`${cardDate(future)} - ${cardDate(future)}`);
      expect(card.priorityIcon).toContain("circle-alert");
      plainColor = card.dateColor;
      expect(plainColor).not.toBe("");
    });

    await test.step("an overdue due date highlights", async () => {
      const card = await driver.hoverCardRead(OVERDUE);
      expect(card.text).toContain(OVERDUE);
      expect(card.text).toContain(cardDate(yesterday));
      expect(card.priorityIcon).toContain("ban");
      expect(card.dateColor).not.toBe("");
      expect(card.dateColor).not.toBe(plainColor);
    });

    await test.step("cleanup: restore the list layout and remove the issues", async () => {
      // The layout persists server-side for every stack user, so never
      // leave the calendar behind.
      await driver.switchIssueLayout("List Layout");
      const current = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      for (const title of [RANGE, OVERDUE, NODATE]) {
        const mine = current.find((issue) => issue.name === title);
        expect(mine).toBeDefined();
        await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
      }
    });
  }
);

test(
  specTitle(["ISS-134"], "archive a completed issue after confirmation"),
  { tag: specTags(["ISS-134"]) },
  async ({ driver, seed }) => {
    const NAME = `NF120 archive me ${Date.now()}`;
    await signInAndOpenIssues(driver, seed);
    const session = await signInSession(seed.email, seed.password);

    let archivedId = "";

    await test.step("setup: a completed state and a completed issue", async () => {
      const states = await serverStates(seed.workspaceSlug, seed.projectId, session);
      let done = states.find((state) => state.group === "completed");
      if (!done) {
        done = await createServerState(seed.workspaceSlug, seed.projectId, session, {
          name: "NF120 Done",
          group: "completed",
          color: "#0E9F6E",
        });
      }
      await createIssue(driver, session, seed, NAME);
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME)!;
      archivedId = mine.id;
      await patchServerIssue(seed.workspaceSlug, seed.projectId, mine.id, session, { state_id: done.id });
      await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
      for (let attempt = 0; attempt < 4; attempt++) {
        const shown = await expect
          .poll(() => driver.visibleIssueNames(), { timeout: 20_000 })
          .toEqual(expect.arrayContaining([...seed.issueNames, NAME]))
          .then(() => true)
          .catch(() => false);
        if (shown) break;
        // The new row sorts last and hides behind group pagination.
        await driver.expandListRows().catch(() => false);
        if (attempt % 2 === 1) await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
      }
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames, NAME]));
    });

    await test.step("the archive modal names the issue and archives it", async () => {
      // The heading carries identifier + sequence: prove the modal is
      // ours before confirming, so a shifted row cannot archive a
      // stranger.
      const ident = (await serverProject(seed.workspaceSlug, seed.projectId, session)).identifier;
      const seq = String(
        (await serverIssueRecord(seed.workspaceSlug, seed.projectId, archivedId, session))["sequence_id"] ?? ""
      );
      await openRowMenuForIssue(driver, NAME, "Archive", () => driver.modalTextContains(`${ident} ${seq}`));
      expect(await driver.modalTextContains("Archive")).toBe(true);
      expect(await driver.modalTextContains("restored later")).toBe(true);
      await driver.confirmArchive();
      await expect.poll(() => driver.pageTextContains("Archive success"), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.visibleIssueNames(), { timeout: 120_000 }).not.toContain(NAME);
    });

    await test.step("the server archived it; cleanup restores and removes", async () => {
      const listed = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      expect(listed.map((issue) => issue.name)).not.toContain(NAME);
      await unarchiveServerIssue(seed.workspaceSlug, seed.projectId, archivedId, session);
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, archivedId, session);
      for (const state of await serverStates(seed.workspaceSlug, seed.projectId, session)) {
        if (state.name === "NF120 Done") await deleteServerState(seed.workspaceSlug, seed.projectId, state.id, session);
      }
    });
  }
);

test(
  specTitle(["ISS-135"], "delete an issue after confirmation"),
  { tag: specTags(["ISS-135"]) },
  async ({ driver, seed }) => {
    const NAME = `NF120 delete me ${Date.now()}`;
    await signInAndOpenIssues(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    await createIssue(driver, session, seed, NAME);

    await test.step("the delete alert warns it is permanent", async () => {
      // The alert carries identifier + sequence: prove the modal is ours
      // before confirming, so a shifted row cannot delete a stranger.
      const mine = (await serverIssues(seed.workspaceSlug, seed.projectId, session)).find(
        (issue) => issue.name === NAME
      )!;
      const ident = (await serverProject(seed.workspaceSlug, seed.projectId, session)).identifier;
      const seq = String(
        (await serverIssueRecord(seed.workspaceSlug, seed.projectId, mine.id, session))["sequence_id"] ?? ""
      );
      await openRowMenuForIssue(driver, NAME, "Delete", () => driver.modalTextContains(`${ident}-${seq}`));
      expect(await driver.modalTextContains("permanently removed")).toBe(true);
      await driver.confirmDeleteIssue();
      // The success toast proves the modal actually deleted (a permission
      // or server error toasts differently and leaves the issue behind).
      await expect.poll(() => driver.pageTextContains("deleted successfully"), { timeout: 30_000 }).toBe(true);
    });

    await test.step("the server no longer holds the issue", async () => {
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .not.toContain(NAME);
    });
  }
);

test(
  specTitle(["ISS-136"], "move modal lists targets and excludes the current project"),
  { tag: specTags(["ISS-136"]) },
  async ({ driver, seed }) => {
    // Sibling runs share the workspace, so other projects may or may not
    // exist: cover both the populated list (with search and exclusion)
    // and the genuine empty state.
    const NAME = `NF120 move me ${Date.now()}`;
    await signInAndOpenIssues(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    await createIssue(driver, session, seed, NAME);
    const others = (await serverProjects(seed.workspaceSlug, session)).filter(
      (project) => project.id !== seed.projectId
    );

    await test.step("targets are offered without the current project", async () => {
      await driver.openRowMenuEntry(NAME, "Move to project");
      expect(await driver.modalTextContains("Move to project")).toBe(true);
      // The search box is an input placeholder, not dialog text.
      expect(await driver.modalHasPlaceholder("Search for a project...")).toBe(true);
      if (others.length === 0) {
        expect(await driver.modalTextContains("No other projects")).toBe(true);
      } else {
        const target = others[0]!.name;
        await driver.fillModalPlaceholder("Search for a project...", target);
        expect(await driver.modalTextContains(target)).toBe(true);
        expect(await driver.modalTextContains(seed.projectName)).toBe(false);
        // A query matching nothing renders the empty state.
        await driver.fillModalPlaceholder("Search for a project...", "zzz-no-such-project");
        expect(await driver.modalTextContains("No other projects")).toBe(true);
      }
      await driver.pressKey("Escape");
    });

    await test.step("cleanup removes only this run's issue", async () => {
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
    });
  }
);
