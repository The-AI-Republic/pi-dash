// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: create/edit work-item modal, core flows (NEWFRONT-120).
// Rows ISS-117–ISS-131 except the row-menu and picker rows, which live in
// modal-actions.spec.ts. Scenarios create uniquely named issues through the
// UI, assert what the user sees plus the server state, and delete only
// their own issues afterwards. Row-menu flows (edit, archive, delete,
// move), the parent picker, labels, and the hover card are covered
// separately; the gantt selection row (ISS-116) rides with the layout
// switcher once its buttons are observable.
import { test, expect } from "../fixtures";
import {
  browserCookies,
  createServerCycle,
  createServerModule,
  deleteServerCycle,
  deleteServerDraft,
  deleteServerIssue,
  deleteServerModule,
  serverDraftNames,
  serverDrafts,
  serverIssueDetails,
  serverIssueRecord,
  serverIssueNames,
  serverIssues,
  serverListStates,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  serverProjectViews,
  signInFreshUser,
  signInSession,
  uniqueSuffix,
} from "../helpers/api";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import { specTags, specTitle } from "../helpers/tags";

async function kanbanOpenBoardWithRetry(driver: ParityDriver): Promise<void> {
  try {
    await driver.kanbanOpenBoard();
  } catch {
    await driver.kanbanOpenBoard();
  }
}

async function openBoardByState(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string
): Promise<{
  user: Awaited<ReturnType<typeof signInFreshUser>>;
  beforeFilters: Record<string, unknown>;
  beforeProperties: Record<string, unknown>;
}> {
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, user.cookie, {
    display_filters: {
      ...before.displayFilters,
      layout: "kanban",
      order_by: "sort_order",
      sub_group_by: null,
      show_empty_groups: true,
      group_by: "state",
    },
    display_properties: before.displayProperties,
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(user));
  await kanbanOpenBoardWithRetry(driver);
  return { user, beforeFilters: before.displayFilters, beforeProperties: before.displayProperties };
}

test(
  specTitle(["ISS-117", "ISS-131"], "create a work item through the modal"),
  { tag: specTags(["ISS-117", "ISS-131"]) },
  async ({ driver, seed }) => {
    const NAME = "NF120 create core";
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();

    await test.step("the modal shows the core form", async () => {
      await driver.openCreateModal();
      expect(await driver.createModalHeading()).toContain("Create new work item");
      expect(await driver.createTitleFocused()).toBe(true);
      expect(await driver.modalPrimaryButtonLabel()).toContain("Save");
      for (const field of ["Parity Project", "Assignees", "Labels", "Start date", "Due date", "Add parent"]) {
        expect(await driver.modalTextContains(field)).toBe(true);
      }
    });

    await test.step("the properties render as an ordered chip row", async () => {
      // NEWPIDASH-10: one wrapping chip row, no captions. The fixed chips
      // show their placeholder or value; the conditional chips track the
      // live project flags (sibling suites toggle them on the shared
      // stack), so each is asserted against its own placeholder text.
      for (const placeholder of ["None", "Assignees", "Labels", "Start date", "Due date", "Add parent"]) {
        expect(await driver.modalTextContains(placeholder)).toBe(true);
      }
      const order = await driver.modalTabOrder();
      const present = (prefix: string): boolean => order.some((entry) => entry.startsWith(prefix));
      for (const stop of ["button#4:", "button#5:", "button#6:", "div#7:", "div#8:", "div#9:"]) {
        expect(present(stop)).toBe(true);
      }
      expect(present("button#10:")).toBe(await driver.modalTextContains("Cycle"));
      expect(present("button#11:")).toBe(await driver.modalTextContains("Modules"));
      // The estimate stop is a button on the new form and a div on the
      // old one; either tag counts.
      const stop12 = order.some((entry) => entry.startsWith("button#12:") || entry.startsWith("div#12:"));
      expect(stop12).toBe(await driver.modalTextContains("Estimate"));
      const stops = order
        .filter((entry) => /^(button|div)#(4|5|6|7|8|9|10|11|12):/.test(entry))
        .map((entry) => Number(entry.replace(/^(?:button|div)#(\d+):.*$/, "$1")));
      expect([...stops].sort((a, b) => a - b)).toEqual(stops);
    });

    const session = await signInSession(seed.email, seed.password);

    await test.step("saving creates the issue with a view/copy toast", async () => {
      await driver.fillCreateTitle(NAME);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      // The toast library renders without landmark roles, so the scenarios
      // assert the user-visible success sentence and its actions by text.
      await expect
        .poll(() => driver.pageTextContains("Work item created successfully"), { timeout: 30_000 })
        .toBe(true);
      expect(await driver.pageTextContains("View work item")).toBe(true);
      expect(await driver.pageTextContains("Copy link")).toBe(true);
    });

    await test.step("the view action opens the new issue", async () => {
      const url = await driver.openToastViewAction();
      expect(url).toContain("/browse/PAR-");
    });

    await test.step("the server stored the issue", async () => {
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .toContain(NAME);
    });

    await test.step("cleanup removes only this run's issue", async () => {
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .not.toContain(NAME);
    });
  }
);

test(
  specTitle(["ISS-118"], "title is required and capped at 255 characters"),
  { tag: specTags(["ISS-118"]) },
  async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();
    await driver.openCreateModal();
    const session = await signInSession(seed.email, seed.password);
    const before = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);

    await test.step("an empty title blocks saving with an inline error", async () => {
      await driver.submitCreateModal();
      expect(await driver.createTitleError()).not.toBe("");
      expect(await driver.createModalOpen()).toBe(true);
    });

    await test.step("a 256-character title is rejected", async () => {
      await driver.fillCreateTitle("n".repeat(256));
      await driver.submitCreateModal();
      expect(await driver.createTitleError()).not.toBe("");
      expect(await driver.createModalOpen()).toBe(true);
    });

    await test.step("whitespace alone is not a title", async () => {
      await driver.fillCreateTitle("   ");
      await driver.submitCreateModal();
      expect(await driver.createTitleError()).not.toBe("");
    });

    await test.step("the server stored nothing new", async () => {
      const after = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(after).toEqual(expect.arrayContaining(before));
      expect(after).not.toContain("n".repeat(256));
    });
  }
);

test(
  specTitle(["ISS-119"], "create more keeps the modal open and resets it"),
  { tag: specTags(["ISS-119"]) },
  async ({ driver, seed }) => {
    const FIRST = "NF120 create-more first";
    const SECOND = "NF120 create-more second";
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();
    await driver.openCreateModal();
    const session = await signInSession(seed.email, seed.password);

    await test.step("with create more on, saving keeps the modal open", async () => {
      await driver.enableCreateMore();
      await driver.fillCreateTitle(FIRST);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createTitleValue(), { timeout: 30_000 }).toBe("");
      expect(await driver.createModalOpen()).toBe(true);
      expect(await driver.createTitleFocused()).toBe(true);
    });

    await test.step("a second issue saves from the kept-open modal", async () => {
      await driver.fillCreateTitle(SECOND);
      await driver.submitCreateModal();
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .toContain(SECOND);
      expect(await driver.createModalOpen()).toBe(true);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("cleanup removes only this run's issues", async () => {
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      for (const name of [FIRST, SECOND]) {
        const mine = all.find((issue) => issue.name === name);
        expect(mine).toBeDefined();
        await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
      }
    });
  }
);

test(
  specTitle(["ISS-119"], "create more from a board column keeps the column state"),
  { tag: specTags(["ISS-119"]) },
  async ({ driver, seed }) => {
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const FIRST = `NF119 prefill first ${suffix}`;
    const SECOND = `NF119 prefill second ${suffix}`;
    const board = await openBoardByState(driver, seed, seed.projectId);
    const session = board.user.cookie;

    await test.step("the state columns render", async () => {
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).not.toHaveLength(0);
    });

    await test.step("the column create opens prefilled with that state", async () => {
      const states = await serverListStates(seed.workspaceSlug, seed.projectId, session);
      const defaultState = states.find((row) => row.isDefault) ?? states[0];
      if (defaultState === undefined) throw new Error("[parity] seed project carries no states.");
      const columns = await driver.kanbanColumns();
      const target = columns.find((column) => column.name !== "None" && column.name !== defaultState.name);
      if (target === undefined) throw new Error("[parity] no non-default state column to prefill from.");
      const targetState = states.find((row) => row.name === target.name);
      if (targetState === undefined)
        throw new Error(`[parity] column ${JSON.stringify(target.name)} has no matching state.`);
      expect(await driver.kanbanHeaderCreateVisible(target.name)).toBe(true);
      await driver.kanbanHeaderCreate(target.name);
      await expect.poll(() => driver.kanbanCreateModalVisible(), { timeout: 30_000 }).toBe(true);
      await driver.enableCreateMore();
      await driver.fillCreateTitle(FIRST);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createTitleValue(), { timeout: 30_000 }).toBe("");
      expect(await driver.createModalOpen()).toBe(true);
      // The second form keeps the opener state: proven through submit so
      // it checks what the server stored, not only what the modal shows.
      await driver.fillCreateTitle(SECOND);
      await driver.submitCreateModal();
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .toContain(SECOND);
      expect(await driver.createModalOpen()).toBe(true);
      for (const name of [FIRST, SECOND]) {
        const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
        const mine = all.find((issue) => issue.name === name);
        expect(mine).toBeDefined();
        const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, mine!.id, session);
        expect(details.stateId).toBe(targetState.id);
      }
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("cleanup removes only this run's issues and restores prefs", async () => {
      for (const name of [FIRST, SECOND]) {
        const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
        const mine = all.find((issue) => issue.name === name);
        if (mine !== undefined)
          await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine.id, session).catch(() => {});
      }
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: board.beforeFilters,
        display_properties: board.beforeProperties,
      }).catch(() => {});
    });
  }
);

test(
  specTitle(["ISS-120"], "discarding a dirty create offers saving a draft"),
  { tag: specTags(["ISS-120"]) },
  async ({ driver, seed }) => {
    const NAME = "NF120 discard to draft";
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();
    const session = await signInSession(seed.email, seed.password);

    await test.step("discarding a pristine create just closes", async () => {
      await driver.openCreateModal();
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      expect(await driver.pageTextContains("Save this draft?")).toBe(false);
    });

    await test.step("discarding a dirty create asks about the draft", async () => {
      await driver.openCreateModal();
      await driver.fillCreateTitle(NAME);
      await driver.clickModalDiscard();
      // The confirm renders asynchronously after the click; poll for it
      // instead of reading once, or loaded runs miss it.
      await expect.poll(() => driver.pageTextContains("Save this draft?"), { timeout: 15_000 }).toBe(true);
      await driver.confirmSaveDraft();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("the draft landed on the server, not the project list", async () => {
      await expect.poll(() => serverDraftNames(seed.workspaceSlug, session), { timeout: 60_000 }).toContain(NAME);
      const issues = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(issues).not.toContain(NAME);
    });

    await test.step("cleanup removes only this run's draft", async () => {
      const all = await serverDrafts(seed.workspaceSlug, session);
      const mine = all.find((draft) => draft.name === NAME);
      expect(mine).toBeDefined();
      await deleteServerDraft(seed.workspaceSlug, mine!.id, session);
    });
  }
);

test(
  specTitle(["ISS-121"], "no duplicate detection runs in this build"),
  { tag: specTags(["ISS-121"]) },
  async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();
    await driver.openCreateModal();

    await test.step("typing a seeded title surfaces no duplicate UI", async () => {
      await driver.fillCreateTitle(seed.issueNames[0]!);
      await new Promise((resolve) => setTimeout(resolve, 3000));
      expect(await driver.modalTextContains("Duplicate")).toBe(false);
      await driver.clickModalDiscard();
    });
  }
);

test(
  specTitle(["ISS-123"], "project selector offers the create-permission project"),
  { tag: specTags(["ISS-123"]) },
  async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();
    await driver.openCreateModal();

    await test.step("the modal is scoped to the seeded project", async () => {
      expect(await driver.modalTextContains(seed.projectName)).toBe(true);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });
  }
);

test(
  specTitle(["ISS-124"], "creating from a module page links the module implicitly"),
  { tag: specTags(["ISS-124"]) },
  async ({ driver, seed }) => {
    // The module page opens the same reduced modal as the cycle page: no
    // cycle/modules pickers, so the route context stays invisible and only
    // materializes on save. (One early probe caught a full picker row
    // mid-render; four settled reads since agree on the reduced form.)
    const STAMP = Date.now();
    const MODULE = `NF120 ctx module ${STAMP}`;
    const NAME = `NF120 module context ${STAMP}`;
    const session = await signInSession(seed.email, seed.password);
    const today = new Date().toISOString().slice(0, 10);
    const module = await createServerModule(seed.workspaceSlug, seed.projectId, session, {
      name: MODULE,
      start_date: today,
      target_date: today,
    });
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);

    await test.step("the modal carries no visible module context", async () => {
      await driver.openModulePage(seed.workspaceSlug, seed.projectId, module.id);
      // The page itself shows the module; only the modal hides it.
      await expect.poll(() => driver.pageTextContains(MODULE), { timeout: 90_000 }).toBe(true);
      await driver.openCreateModal();
      expect(await driver.modalTextContains(MODULE)).toBe(false);
      expect(await driver.modalTextContains("Cycle")).toBe(false);
      expect(await driver.modalTextContains("Modules")).toBe(false);
    });

    await test.step("saving links the issue to the module", async () => {
      await driver.fillCreateTitle(NAME);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      // The new app attaches route membership in a bridge write after
      // the create lands; poll for it rather than reading once (gate:
      // a one-shot read flaked red under host load, ISS-124).
      await expect
        .poll(
          async () =>
            ((await serverIssueRecord(seed.workspaceSlug, seed.projectId, mine!.id, session))["module_ids"] ??
              []) as unknown[],
          { timeout: 30_000 }
        )
        .toContain(module.id);
      const detail = await serverIssueRecord(seed.workspaceSlug, seed.projectId, mine!.id, session);
      expect(detail["cycle_id"] ?? null).toBeNull();
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
    });

    await test.step("cleanup removes the module", async () => {
      await deleteServerModule(seed.workspaceSlug, seed.projectId, module.id, session);
    });
  }
);

test(
  specTitle(["ISS-124"], "creating from a cycle page links the cycle implicitly"),
  { tag: specTags(["ISS-124"]) },
  async ({ driver, seed }) => {
    // Unlike the module page — whose modal visibly pre-seeds the module
    // chip — the cycle page opens a modal with no cycle/modules pickers at
    // all: the route context stays implicit and only materializes on save.
    const STAMP = Date.now();
    const CYCLE = `NF120 ctx cycle ${STAMP}`;
    const NAME = `NF120 cycle context ${STAMP}`;
    const session = await signInSession(seed.email, seed.password);
    const cycle = await createServerCycle(seed.workspaceSlug, seed.projectId, session, {
      name: CYCLE,
      start_date: new Date().toISOString(),
      end_date: new Date(Date.now() + 7 * 86400000).toISOString(),
    });
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);

    await test.step("the modal carries no visible cycle context", async () => {
      await driver.openCyclePage(seed.workspaceSlug, seed.projectId, cycle.id);
      await driver.openCreateModal();
      expect(await driver.modalTextContains(CYCLE)).toBe(false);
      expect(await driver.modalTextContains("Cycle")).toBe(false);
      expect(await driver.modalTextContains("Modules")).toBe(false);
    });

    await test.step("saving links the issue to the cycle", async () => {
      await driver.fillCreateTitle(NAME);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      // Same bridge-write race as the module leg (gate, ISS-124).
      await expect
        .poll(
          async () =>
            String((await serverIssueRecord(seed.workspaceSlug, seed.projectId, mine!.id, session))["cycle_id"] ?? ""),
          { timeout: 30_000 }
        )
        .toBe(cycle.id);
      const detail = await serverIssueRecord(seed.workspaceSlug, seed.projectId, mine!.id, session);
      expect(((detail["module_ids"] ?? []) as unknown[]).length).toBe(0);
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
    });

    await test.step("cleanup removes the cycle", async () => {
      await deleteServerCycle(seed.workspaceSlug, seed.projectId, cycle.id, session);
    });
  }
);

test(
  specTitle(["ISS-124"], "creating from the project page leaves cycle and modules empty"),
  { tag: specTags(["ISS-124"]) },
  async ({ driver, seed }) => {
    const NAME = "NF120 no cycle context";
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();
    await driver.openCreateModal();
    const session = await signInSession(seed.email, seed.password);

    await test.step("saving stores no cycle or module links", async () => {
      await driver.fillCreateTitle(NAME);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      const detail = await serverIssueRecord(seed.workspaceSlug, seed.projectId, mine!.id, session);
      expect(detail["cycle_id"] ?? null).toBeNull();
      const modules = (detail["module_ids"] ?? []) as unknown[];
      expect(modules.length).toBe(0);
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
    });
  }
);

test(
  specTitle(["ISS-125"], "escape closes a pristine modal, dirty closes ask first"),
  { tag: specTags(["ISS-125"]) },
  async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();

    await test.step("escape closes the untouched modal", async () => {
      await driver.openCreateModal();
      await driver.pressKey("Escape");
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("escape on a dirty modal raises the draft confirm", async () => {
      await driver.openCreateModal();
      await driver.fillCreateTitle("NF120 escape guard");
      await driver.pressKey("Escape");
      // The confirm renders asynchronously after the keypress; poll for it
      // instead of reading once, or loaded runs miss it.
      await expect.poll(() => driver.pageTextContains("Save this draft?"), { timeout: 15_000 }).toBe(true);
      await driver.cancelDiscardDialog();
      expect(await driver.createModalOpen()).toBe(true);
      await driver.clickModalDiscard();
      await driver.discardDialogDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("the guard draft was discarded, nothing was stored", async () => {
      const session = await signInSession(seed.email, seed.password);
      const issues = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(issues).not.toContain("NF120 escape guard");
      const drafts = await serverDraftNames(seed.workspaceSlug, session);
      expect(drafts).not.toContain("NF120 escape guard");
    });

    await test.step("fields carry an explicit tab-index order", async () => {
      await driver.openCreateModal();
      const order = await driver.modalTabOrder();
      // The title leads the order; pickers and actions follow in a fixed
      // sequence (state/priority/assignee values vary, so match the index).
      expect(order).toContain("input#1:Title");
      expect(order.some((entry) => entry.startsWith("button#4:"))).toBe(true);
      expect(order.some((entry) => entry.startsWith("button#5:"))).toBe(true);
      expect(order.some((entry) => entry.startsWith("button#6:"))).toBe(true);
      expect(order).toContain("div#7:Labels");
      expect(order).toContain("div#8:Start date");
      expect(order).toContain("div#9:Due date");
      // The cycle/modules pickers join the order only while the project's
      // view flags enable them (sibling suites toggle these on the shared
      // stack), so assert their presence against the live flags.
      const session = await signInSession(seed.email, seed.password);
      const views = await serverProjectViews(seed.workspaceSlug, seed.projectId, session);
      if (views.cycleView) expect(order).toContain("button#10:Cycle");
      else expect(order.some((entry) => entry.startsWith("button#10:"))).toBe(false);
      if (views.moduleView) expect(order).toContain("button#11:Modules");
      else expect(order.some((entry) => entry.startsWith("button#11:"))).toBe(false);
      expect(order).toContain("div#15:Discard");
      expect(order).toContain("div#16:Save");
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("enter in the title submits the modal", async () => {
      const NAME = `NF120 enter submits ${Date.now()}`;
      await driver.openCreateModal();
      await driver.fillCreateTitle(NAME);
      await driver.fillDescription("entered body");
      await driver.focusCreateTitle();
      await driver.pressKey("Enter");
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      const session = await signInSession(seed.email, seed.password);
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .toContain(NAME);
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
    });

    await test.step("enter in the description does not submit", async () => {
      const NAME = `NF120 enter newline ${Date.now()}`;
      await driver.openCreateModal();
      await driver.fillCreateTitle(NAME);
      await driver.fillDescription("first line");
      // Focus stays in the editor after typing, so this Enter lands there.
      await driver.pressKey("Enter");
      await new Promise((resolve) => setTimeout(resolve, 3000));
      expect(await driver.createModalOpen()).toBe(true);
      expect(await driver.createTitleValue()).toBe(NAME);
      const session = await signInSession(seed.email, seed.password);
      const issues = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(issues).not.toContain(NAME);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.pageTextContains("Save this draft?"), { timeout: 15_000 }).toBe(true);
      await driver.discardDialogDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });
  }
);

test(
  specTitle(["ISS-127"], "description text round-trips through create"),
  { tag: specTags(["ISS-127"]) },
  async ({ driver, seed }) => {
    const NAME = "NF120 description round-trip";
    const BODY = "nf120 description body text";
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();
    await driver.openCreateModal();
    const session = await signInSession(seed.email, seed.password);

    await test.step("typed description is stored with the issue", async () => {
      await driver.fillCreateTitle(NAME);
      await driver.fillDescription(BODY);
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      const detail = await serverIssueRecord(seed.workspaceSlug, seed.projectId, mine!.id, session);
      expect(String(detail["description_html"] ?? "")).toContain(BODY);
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
    });
  }
);

test(
  specTitle(["ISS-128"], "bug: NEWFRONT-140 git work branch stays optional and validated"),
  { tag: specTags(["ISS-128"]) },
  async ({ driver, seed }) => {
    const NAME = "NF120 git branch";
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();
    await driver.openCreateModal();
    const session = await signInSession(seed.email, seed.password);

    await test.step("the advanced section starts collapsed", async () => {
      expect(await driver.modalTextContains("Work branch")).toBe(false);
      await driver.toggleAdvancedGit();
      expect(await driver.modalTextContains("Work branch")).toBe(true);
    });

    await test.step("a branch with illegal characters is rejected", async () => {
      await driver.fillGitBranch("bad branch!");
      await driver.fillCreateTitle(NAME);
      await driver.submitCreateModal();
      expect(await driver.gitBranchError()).not.toBe("");
      expect(await driver.createModalOpen()).toBe(true);
    });

    await test.step("a valid branch saves with the issue", async () => {
      await driver.fillGitBranch("nf120/branch-1");
      await driver.submitCreateModal();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .toContain(NAME);
    });

    await test.step("the branch never reads back (bug NEWFRONT-140)", async () => {
      // The server stores the value (verified in the database) but no read
      // returns it, so the edit modal always shows an empty branch field.
      // Intended: the saved value round-trips into the edit modal.
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME)!;
      const detail = await serverIssueRecord(seed.workspaceSlug, seed.projectId, mine.id, session);
      expect(detail["git_work_branch"] ?? null).toBeNull();
      await driver.openRowMenuEntry(NAME, "Edit");
      if (!(await driver.modalTextContains("Work branch"))) await driver.toggleAdvancedGit();
      expect(await driver.gitBranchValue()).toBe("");
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("cleanup removes only this run's issue", async () => {
      const again = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const kept = again.find((issue) => issue.name === NAME);
      expect(kept).toBeDefined();
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, kept!.id, session);
    });
  }
);

test(
  specTitle(["ISS-129"], "no type, template, or extra properties in this build"),
  { tag: specTags(["ISS-129"]) },
  async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
    await driver.dismissWelcomeDialog();
    await driver.openCreateModal();

    await test.step("the modal has no type or template pickers", async () => {
      expect(await driver.modalTextContains("Template")).toBe(false);
      expect(await driver.modalTextContains("Issue type")).toBe(false);
      await driver.clickModalDiscard();
      await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
    });
  }
);

test(
  specTitle(["ISS-130"], "a draft publishes into the project"),
  { tag: specTags(["ISS-130"]) },
  async ({ driver, seed }) => {
    const NAME = "NF120 publish me";
    await driver.openEntry();
    await driver.signInWithPasswordRetry(seed.email, seed.password);
    await driver.openDraftsPage(seed.workspaceSlug);
    await driver.dismissWelcomeDialog();
    const session = await signInSession(seed.email, seed.password);

    await test.step("save a draft through the draft modal", async () => {
      // Never reuse a same-name draft: a stale one can be linked to a
      // foreign project, and the draft modal's default project drifts on
      // the shared stack, so always recreate with the seed project pinned
      // (the publish step asserts the issue lands in the seed project).
      const stale = (await serverDrafts(seed.workspaceSlug, session)).find((draft) => draft.name === NAME);
      if (stale) await deleteServerDraft(seed.workspaceSlug, stale.id, session);
      await driver.openCreateDraftModal();
      expect(await driver.modalPrimaryButtonLabel()).toContain("Draft");
      if ((await driver.modalProjectName()) !== seed.projectName) {
        await driver.selectModalProject(seed.projectName);
      }
      await driver.fillCreateTitle(NAME);
      await driver.submitCreateModal();
      await expect.poll(() => driver.visibleDraftNames(), { timeout: 120_000 }).toContain(NAME);
    });

    await test.step("publishing moves it to the project list", async () => {
      await driver.openDraftForEdit(NAME);
      await driver.publishDraft();
      await expect.poll(() => driver.pageTextContains("Draft published to project."), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => serverDraftNames(seed.workspaceSlug, session), { timeout: 60_000 }).not.toContain(NAME);
      await expect
        .poll(() => serverIssueNames(seed.workspaceSlug, seed.projectId, session), { timeout: 60_000 })
        .toContain(NAME);
    });

    await test.step("cleanup removes only the published issue", async () => {
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const mine = all.find((issue) => issue.name === NAME);
      expect(mine).toBeDefined();
      await deleteServerIssue(seed.workspaceSlug, seed.projectId, mine!.id, session);
    });
  }
);
