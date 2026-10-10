// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-222): archived work-items list surface — the
// tab strip with cycles/modules gating, the breadcrumb trail with the
// archived-count badge, per-tab browser titles, the read-only list with
// row menus, the shared filter expression with removable chips, the
// Display control (grouping, ordering, columns), and the side peek panel
// with locked fields and address sync.
// Rows: ARCH-001..ARCH-007.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverArchiveIssue,
  serverArchivedIssues,
  serverArchivesProjectFlags,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverDeleteState,
  serverIssue,
  serverPatchIssue,
  serverPatchProject,
  serverProjectStates,
  serverUnarchiveIssue,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test.setTimeout(480_000);
const LIST_POLL = { timeout: 120_000 };

type SeedFacts = {
  email: string;
  password: string;
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  issueNames: string[];
};

/** A completed/cancelled state to archive from, minting one when the project has none. */
async function doneStateFor(
  seed: SeedFacts,
  projectId: string,
  session: string,
  tag: string
): Promise<{ id: string; name: string; owned: string | null }> {
  const states = await serverProjectStates(seed.workspaceSlug, projectId, session);
  const seededDone = states.find((s) => s.group === "completed" || s.group === "cancelled");
  if (seededDone !== undefined) return { id: seededDone.id, name: seededDone.name, owned: null };
  const owned = await serverCreateState(seed.workspaceSlug, projectId, `${tag} done`, "completed", session);
  const reread = await serverProjectStates(seed.workspaceSlug, projectId, session);
  return { id: owned, name: reread.find((s) => s.id === owned)?.name ?? `${tag} done`, owned };
}

/** Create an issue, move it to `stateId`, and archive it through the API. */
async function archiveNamedIssue(
  seed: SeedFacts,
  projectId: string,
  name: string,
  stateId: string,
  session: string,
  extraPatch: Record<string, unknown> = {}
): Promise<{ id: string; name: string }> {
  const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, name, session);
  await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: stateId, ...extraPatch }, session);
  await serverArchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
  return issue;
}

/** Undo archiveNamedIssue plus its scratch project/states (lenient: teardown never fails a scenario). */
async function cleanupArchivedFixture(
  seed: SeedFacts,
  projectId: string,
  issueIds: string[],
  ownedStateIds: (string | null)[],
  session: string
): Promise<void> {
  for (const issueId of issueIds) {
    await serverUnarchiveIssue(seed.workspaceSlug, projectId, issueId, session).catch(() => {});
    await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issueId, session).catch(() => {});
  }
  // States delete only once no issue references them; the project delete
  // cascades anything left behind.
  for (const stateId of ownedStateIds) {
    if (stateId !== null) await serverDeleteState(seed.workspaceSlug, projectId, stateId, session).catch(() => {});
  }
  await serverCleanupProject(seed.workspaceSlug, projectId, session).catch(() => {});
}

test(
  specTitle(["ARCH-001"], "archive tabs hide cycles and modules while the project keeps them off"),
  { tag: specTags(["ARCH-001"]) },
  async ({ driver, seed }) => {
    const tag = `NF222 gating ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectName = `${tag} project`;
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectName,
      parityProjectIdentifier("N222"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("both flags off: only the work-items tab renders", async () => {
        const flags = await serverArchivesProjectFlags(seed.workspaceSlug, projectId, session);
        expect(flags).toEqual({ cycleView: false, moduleView: false });
        await driver.archivesOpenIssuesList(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.archivesTabNames(), LIST_POLL).toEqual(["Work items"]);
        expect(await driver.archivesActiveTab()).toBe("Work items");
      });

      await test.step("enabling the flags reveals both tabs after reload", async () => {
        await serverPatchProject(seed.workspaceSlug, projectId, session, { cycle_view: true, module_view: true });
        const flags = await serverArchivesProjectFlags(seed.workspaceSlug, projectId, session);
        expect(flags).toEqual({ cycleView: true, moduleView: true });
        await driver.page.reload();
        await expect.poll(() => driver.archivesTabNames(), LIST_POLL).toEqual(["Work items", "Cycles", "Modules"]);
      });

      await test.step("disabling one flag hides just that tab", async () => {
        await serverPatchProject(seed.workspaceSlug, projectId, session, { cycle_view: false });
        const flags = await serverArchivesProjectFlags(seed.workspaceSlug, projectId, session);
        expect(flags).toEqual({ cycleView: false, moduleView: true });
        await driver.page.reload();
        await expect.poll(() => driver.archivesTabNames(), LIST_POLL).toEqual(["Work items", "Modules"]);
      });
    } finally {
      await cleanupArchivedFixture(seed, projectId, [], [], session);
    }
  }
);

test(
  specTitle(["ARCH-001"], "tab strip navigates between archive tabs with the active tab marked"),
  { tag: specTags(["ARCH-001"]) },
  async ({ driver, seed }) => {
    const tag = `NF222 tabnav ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N222"),
      { cycleView: true, moduleView: true },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesOpenIssuesList(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.archivesTabNames(), LIST_POLL).toEqual(["Work items", "Cycles", "Modules"]);

      await test.step("each tab navigates to its own address and becomes active", async () => {
        expect(await driver.archivesActiveTab()).toBe("Work items");
        await driver.archivesOpenTab("cycles");
        expect(driver.page.url()).toContain(`/projects/${projectId}/archives/cycles`);
        expect(await driver.archivesActiveTab()).toBe("Cycles");
        await driver.archivesOpenTab("modules");
        expect(driver.page.url()).toContain(`/projects/${projectId}/archives/modules`);
        expect(await driver.archivesActiveTab()).toBe("Modules");
        await driver.archivesOpenTab("issues");
        expect(driver.page.url()).toContain(`/projects/${projectId}/archives/issues`);
        expect(await driver.archivesActiveTab()).toBe("Work items");
      });

      await test.step("the server still reports both flags on", async () => {
        expect(await serverArchivesProjectFlags(seed.workspaceSlug, projectId, session)).toEqual({
          cycleView: true,
          moduleView: true,
        });
      });
    } finally {
      await cleanupArchivedFixture(seed, projectId, [], [], session);
    }
  }
);

test(
  specTitle(["ARCH-002"], "breadcrumb trail, count badge, and the way back toward the project"),
  { tag: specTags(["ARCH-002"]) },
  async ({ driver, seed }) => {
    const tag = `NF222 crumb ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectName = `${tag} project`;
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectName,
      parityProjectIdentifier("N222"),
      {},
      session
    );
    const done = await doneStateFor(seed, projectId, session, tag);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesOpenIssuesList(seed.workspaceSlug, projectId);

      await test.step("the trail names project, archives, and the active tab", async () => {
        // The project crumb is a switcher whose truncation mark ("...") may
        // prefix the name depending on the header width; match the name
        // itself, not the responsive chrome around it.
        await expect
          .poll(() => driver.breadcrumbLabels(), LIST_POLL)
          .toEqual([expect.stringContaining(projectName), "Archives", "Work items"]);
      });

      await test.step("zero archived items: no badge, and the server agrees", async () => {
        expect(await serverArchivedIssues(seed.workspaceSlug, projectId, session)).toEqual([]);
        expect(await driver.archivesCountBadge()).toBeNull();
        expect(await driver.archivesCountBadgeTooltip()).toBeNull();
      });

      await test.step("archived items raise the badge with its hover tip", async () => {
        const first = await archiveNamedIssue(seed, projectId, `${tag} alpha`, done.id, session);
        const second = await archiveNamedIssue(seed, projectId, `${tag} beta`, done.id, session);
        try {
          expect(await serverArchivedIssues(seed.workspaceSlug, projectId, session)).toHaveLength(2);
          await driver.page.reload();
          await expect.poll(() => driver.archivesCountBadge(), LIST_POLL).toBe("2");
          const tip = await driver.archivesCountBadgeTooltip();
          expect(tip ?? "").toContain("2");
          expect(tip ?? "").toMatch(/work items?/);
        } finally {
          await serverUnarchiveIssue(seed.workspaceSlug, projectId, first.id, session).catch(() => {});
          await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, first.id, session).catch(() => {});
          await serverUnarchiveIssue(seed.workspaceSlug, projectId, second.id, session).catch(() => {});
          await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, second.id, session).catch(() => {});
        }
      });

      await test.step("the trail leads back toward the project", async () => {
        // The archived list emptied again above; the trail still resolves.
        await driver.page.reload();
        await expect
          .poll(() => driver.breadcrumbLabels(), LIST_POLL)
          .toEqual([expect.stringContaining(projectName), "Archives", "Work items"]);
        // The Archives crumb links back to the archives root from a tab.
        await driver.page.getByRole("link", { name: "Archives" }).first().click();
        await expect.poll(() => driver.page.url(), LIST_POLL).toContain(`/projects/${projectId}/archives/issues`);
      });

      await test.step("a back control returns to the previous screen on narrow screens", async () => {
        // The back affordance renders only at viewport widths of 640px and
        // below; the desktop header carries the trail but no back control.
        expect(await driver.archivesBackPresent()).toBe(false);
        await driver.page.setViewportSize({ width: 600, height: 800 });
        await driver.openProjectIssues(seed.workspaceSlug, projectId);
        await driver.archivesOpenIssuesList(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.archivesBackPresent(), LIST_POLL).toBe(true);
        await driver.archivesClickBack();
        await driver.page.waitForURL(`**/projects/${projectId}/issues**`, { timeout: 60_000 });
      });
    } finally {
      await cleanupArchivedFixture(seed, projectId, [], [done.owned], session);
    }
  }
);

test(
  specTitle(["ARCH-003"], "browser title names the project and archive section per tab"),
  { tag: specTags(["ARCH-003"]) },
  async ({ driver, seed }) => {
    const tag = `NF222 titles ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectName = `${tag} project`;
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectName,
      parityProjectIdentifier("N222"),
      { cycleView: true, moduleView: true },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesOpenIssuesList(seed.workspaceSlug, projectId);

      for (const [tab, section] of [
        ["issues", "work items"],
        ["cycles", "cycles"],
        ["modules", "modules"],
      ] as const) {
        await test.step(`the ${section} tab titles the project plus the section`, async () => {
          await driver.archivesOpenTab(tab);
          await expect.poll(() => driver.archivesPageTitle(), LIST_POLL).toBe(`${projectName} - Archived ${section}`);
        });
      }
    } finally {
      await cleanupArchivedFixture(seed, projectId, [], [], session);
    }
  }
);

test(
  specTitle(
    ["ARCH-004"],
    "bug: NEWFRONT-234 archived row menu omits Delete; list is read-only with restore, open, and copy-link"
  ),
  { tag: specTags(["ARCH-004"]) },
  async ({ driver, seed }) => {
    const tag = `NF222 readonly ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N222"),
      {},
      session
    );
    const done = await doneStateFor(seed, projectId, session, tag);
    const first = await archiveNamedIssue(seed, projectId, `${tag} alpha`, done.id, session);
    const second = await archiveNamedIssue(seed, projectId, `${tag} beta`, done.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesOpenIssuesList(seed.workspaceSlug, projectId);

      await test.step("the server holds both archived items", async () => {
        const rows = await serverArchivedIssues(seed.workspaceSlug, projectId, session);
        expect(rows.map((row) => row.name).sort()).toEqual([first.name, second.name].sort());
      });

      await test.step("the list renders the archived items", async () => {
        await expect
          .poll(() => driver.archivesVisibleIssueNames(), LIST_POLL)
          .toEqual(expect.arrayContaining([first.name, second.name]));
      });

      await test.step("no cell opens an inline editor", async () => {
        expect(await driver.archivesInlineEditorOpens(first.name)).toBe(false);
        // The probe may leave the row menu open; close it so the next
        // step starts from a settled page.
        await driver.archivesCloseMenus();
      });

      await test.step("each row offers restore, open, and copy-link — but no Delete (bug NEWFRONT-234)", async () => {
        await driver.archivesOpenRowMenu(second.name);
        try {
          // Live menu carries three entries: the read-only gate hides the
          // wired Delete entry (NEWFRONT-234). Intended: Delete renders
          // fourth and opens the delete confirmation.
          expect(await driver.archivesRowMenuEntries()).toEqual(["Restore", "Open in new tab", "Copy link"]);
        } finally {
          await driver.archivesCloseMenus();
        }
      });
    } finally {
      await cleanupArchivedFixture(seed, projectId, [first.id, second.id], [done.owned], session);
    }
  }
);

test(
  specTitle(
    ["ARCH-005"],
    "bug: NEWFRONT-242 seeded filter chips narrow the archived list; removing and clearing re-run the list query"
  ),
  { tag: specTags(["ARCH-005"]) },
  async ({ driver, seed }) => {
    const tag = `NF222 filters ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N222"),
      {},
      session
    );
    const done = await doneStateFor(seed, projectId, session, tag);
    const urgent = await archiveNamedIssue(seed, projectId, `${tag} urgent`, done.id, session, { priority: "urgent" });
    const low = await archiveNamedIssue(seed, projectId, `${tag} low`, done.id, session, { priority: "low" });
    const none = await archiveNamedIssue(seed, projectId, `${tag} none`, done.id, session, { priority: "none" });
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesOpenIssuesList(seed.workspaceSlug, projectId);
      await expect
        .poll(() => driver.archivesVisibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([urgent.name, low.name, none.name]));
      // Fresh project, no stored expression: the chip row hides. The
      // header filter entry that should add the first condition is inert
      // on apps/web (bug NEWFRONT-242), so each step below seeds the
      // stored expression the row reads, then drives chips and clearing
      // through the shared row UI.
      expect(await driver.isRichFilterRowVisible()).toBe(false);

      await test.step("seeded expression renders its chip and narrows the list query", async () => {
        const loadWait = driver.archivesWaitForListQuery();
        await driver.archivesSeedStoredExpression(seed.workspaceSlug, projectId, { and: [{ priority__in: "urgent" }] });
        const loadQuery = await loadWait;
        expect(loadQuery.url).toContain("/archived-issues/");
        expect(loadQuery.params).toEqual(expect.objectContaining({ filters: expect.stringContaining("urgent") }));
        await expect.poll(() => driver.archivesVisibleIssueNames(), LIST_POLL).toEqual([urgent.name]);
        expect(await driver.isRichFilterRowVisible()).toBe(true);
        expect(await driver.richFilterRowText()).toContain("Urgent");
        expect(await driver.richConditionCount()).toBe(1);
      });

      await test.step("removing the chip re-runs the query without the filter", async () => {
        const queryWait = driver.archivesWaitForListQuery();
        await driver.removeRichCondition(0);
        const query = await queryWait;
        expect(query.url).toContain("/archived-issues/");
        expect(JSON.stringify(query.params)).not.toContain("urgent");
        await expect
          .poll(() => driver.archivesVisibleIssueNames(), LIST_POLL)
          .toEqual(expect.arrayContaining([urgent.name, low.name, none.name]));
      });

      await test.step("clear-all drops every condition and restores the list", async () => {
        // Removing the last condition hid the row again; re-seed from the
        // empty state the same way.
        expect(await driver.isRichFilterRowVisible()).toBe(false);
        const loadWait = driver.archivesWaitForListQuery();
        await driver.archivesSeedStoredExpression(seed.workspaceSlug, projectId, { and: [{ priority__in: "low" }] });
        const loadQuery = await loadWait;
        expect(loadQuery.url).toContain("/archived-issues/");
        await expect.poll(() => driver.archivesVisibleIssueNames(), LIST_POLL).toEqual([low.name]);
        const queryWait = driver.archivesWaitForListQuery();
        await driver.clearRichFilters();
        const query = await queryWait;
        expect(JSON.stringify(query.params)).not.toContain("low");
        await expect
          .poll(() => driver.archivesVisibleIssueNames(), LIST_POLL)
          .toEqual(expect.arrayContaining([urgent.name, low.name, none.name]));
      });
    } finally {
      await cleanupArchivedFixture(seed, projectId, [urgent.id, low.id, none.id], [done.owned], session);
    }
  }
);

test(
  specTitle(["ARCH-006"], "display grouping, ordering, and columns re-render the archived list"),
  { tag: specTags(["ARCH-006"]) },
  async ({ driver, seed }) => {
    // NOTE: the tag must not contain "display" — role-name matching is
    // substring-based and would confuse the Display dropdown trigger.
    const tag = `NF222 layout ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N222"),
      {},
      session
    );
    const done = await doneStateFor(seed, projectId, session, tag);
    const cancelledId = await serverCreateState(seed.workspaceSlug, projectId, `${tag} dropped`, "cancelled", session);
    // Start dates run opposite to creation order so date ordering provably
    // differs from creation ordering below.
    const first = await archiveNamedIssue(seed, projectId, `${tag} alpha`, done.id, session, {
      priority: "urgent",
      start_date: "2026-01-05",
    });
    const second = await archiveNamedIssue(seed, projectId, `${tag} beta`, cancelledId, session, {
      priority: "low",
      start_date: "2026-03-10",
    });
    // Group-by pills toggle and each set resolves asynchronously, so a set
    // fired while the previous one is still in flight can lose the race.
    // Set, then poll for the wanted grouping, re-setting (same value, so
    // retries still converge) up to three times before failing honestly.
    const setGroupBy = async (option: "States" | "None", want: string[]): Promise<void> => {
      let lastError: unknown = null;
      for (let attempt = 0; attempt < 3; attempt++) {
        await driver.setDisplayGroupBy(option);
        try {
          if (want.length === 0) {
            await expect.poll(() => driver.archivesGroupHeadings(), { timeout: 30_000 }).toEqual([]);
          } else {
            await expect
              .poll(() => driver.archivesGroupHeadings(), { timeout: 30_000 })
              .toEqual(expect.arrayContaining(want));
          }
          return;
        } catch (error) {
          lastError = error;
        }
      }
      throw lastError;
    };
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesOpenIssuesList(seed.workspaceSlug, projectId);
      await expect
        .poll(() => driver.archivesVisibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([first.name, second.name]));

      // One panel session for the whole scenario: the panel stays open
      // across the steps below (each step verifies its set before the next
      // set fires, so nothing races), and the scenario never closes it.
      await test.step("the panel offers grouping, ordering, and columns", async () => {
        await driver.openDisplayOptions();
        const panel = await driver.displayPanelText();
        expect(panel).toContain("Group by");
        expect(panel).toContain("Order by");
        expect(panel).toContain("Display Properties");
      });

      await test.step("grouping by state splits the rows; None reunites them", async () => {
        await setGroupBy("States", [done.name, `${tag} dropped`]);
        await setGroupBy("None", []);
        await expect
          .poll(() => driver.archivesVisibleIssueNames(), LIST_POLL)
          .toEqual(expect.arrayContaining([first.name, second.name]));
      });

      await test.step("ordering changes the row order, manual included", async () => {
        await driver.setDisplayOrderBy("Manual");
        await expect.poll(() => driver.isDisplayOptionChecked("Manual"), { timeout: 30_000 }).toBe(true);
        await expect
          .poll(() => driver.archivesVisibleIssueNames(), LIST_POLL)
          .toEqual(expect.arrayContaining([first.name, second.name]));
        // Newest-first creation order opposes start-date order by design.
        await driver.setDisplayOrderBy("Last created");
        await expect.poll(() => driver.archivesVisibleIssueNames(), LIST_POLL).toEqual([second.name, first.name]);
        await driver.setDisplayOrderBy("Start date");
        await expect.poll(() => driver.archivesVisibleIssueNames(), LIST_POLL).toEqual([first.name, second.name]);
      });

      await test.step("toggling a column re-renders the rows", async () => {
        // Start date renders as row text either way the toggle flips, so
        // the rendered row always changes (Priority renders icon-only and
        // would be a text no-op).
        const before = await driver.archivesRowText(first.name);
        await driver.toggleDisplayProperty("Start date");
        await expect.poll(() => driver.archivesRowText(first.name), { timeout: 30_000 }).not.toBe(before);
      });
    } finally {
      await cleanupArchivedFixture(seed, projectId, [first.id, second.id], [done.owned, cancelledId], session);
    }
  }
);

test(
  specTitle(["ARCH-007"], "peek locks every field and carries the selection in the address"),
  { tag: specTags(["ARCH-007"]) },
  async ({ driver, seed }) => {
    const tag = `NF222 peek ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N222"),
      {},
      session
    );
    const done = await doneStateFor(seed, projectId, session, tag);
    const body = `${tag} peek body`;
    const issue = await archiveNamedIssue(seed, projectId, `${tag} item`, done.id, session, {
      description_html: `<p>${body}</p>`,
    });
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesOpenIssuesList(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.archivesVisibleIssueNames(), LIST_POLL).toContain(issue.name);

      await test.step("the server marks the issue archived", async () => {
        const detail = await serverIssue(seed.workspaceSlug, projectId, issue.id, session);
        expect(detail.archived_at).not.toBe(null);
      });

      await test.step("selecting the row opens the peek with locked fields", async () => {
        await driver.clickListRow(issue.name);
        await expect.poll(() => driver.archivesPeekTitle(), LIST_POLL).toBe(issue.name);
        expect(await driver.archivesPeekTitleLocked()).toBe(true);
        expect(await driver.archivesPeekDescriptionText()).toContain(body);
        expect(await driver.archivesPeekDescriptionEditable()).toBe(false);
        for (const label of ["State", "Assignees", "Priority"]) {
          expect(await driver.propertyPickerDisabled(label)).toBe(true);
        }
        expect(await driver.pickerOpen()).toBe(false);
        expect(await driver.archivesPeekActivityEditable()).toBe(false);
      });

      await test.step("the address carries the peeked item", async () => {
        const params = await driver.archivesPeekQueryParams();
        expect(params.issue).toBe(issue.id);
        // Same-project peeks omit the project param; no nesting here.
        expect(params.project).toBeNull();
        expect(params.nesting).toBeNull();
      });

      await test.step("reload reopens the same peek; closing clears the address", async () => {
        await driver.page.reload();
        await expect.poll(() => driver.archivesPeekTitle(), LIST_POLL).toBe(issue.name);
        await driver.closePeek();
        await expect.poll(() => driver.peekOpen(), LIST_POLL).toBe(false);
        const params = await driver.archivesPeekQueryParams();
        expect(params.issue).toBeNull();
        expect(params.project).toBeNull();
        expect(params.nesting).toBeNull();
      });
    } finally {
      await cleanupArchivedFixture(seed, projectId, [issue.id], [done.owned], session);
    }
  }
);
