// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): edition-only features stay absent in
// OSS — every cloud/EE surface below is an empty ce/ stub (verified by
// reading the stub components: each renders an empty fragment), so the
// scenarios assert the absence on the live OSS surfaces plus the
// surrounding OSS behavior that must keep working.
// Rows: ISS-231–ISS-237 (edition-only stubs).
//
// Absence regexes follow the worklog-absent (ISS-206) tradition: the
// feature's own name must not appear in the rendered page text. Absence
// specs keep the marker words out of their own tags — the sidebar lists
// scenario projects in the page text.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddSubIssues,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCleanupView,
  serverCreateCycle,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCreateView,
  serverDeleteCycle,
  serverIssue,
  serverPatchIssue,
  serverProjectStates,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-231"], "no de-dupe UI in the OSS create modal or issue detail"),
  { tag: specTags(["ISS-231"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 dedupe ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await test.step("typing in the create modal surfaces no similar-issue UI", async () => {
        await driver.issueModalOpenCreate(seed.workspaceSlug, projectId);
        await driver.issueModalFillTitle(`${tag} typing probe with a long descriptive title`);
        const modalText = await page.locator("body").innerText();
        expect(modalText).not.toMatch(/duplicat/i);
        await driver.issueModalProjectPressEscape();
      });
      await test.step("the issue detail shows no duplicate popover", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        const detailText = await page.locator("body").innerText();
        expect(detailText).not.toMatch(/duplicat/i);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-232"], "no epic UI in OSS relations and sub-issue flows"),
  { tag: specTags(["ISS-232"]) },
  async ({ driver, seed, page }) => {
    // NOTE: absence specs must keep the marker words out of their own
    // tags — the sidebar lists scenario projects in the page text.
    const tag = `NF122 rel ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    // The epic branch keys off is_epic, which OSS issues never carry; the
    // epic modal stub renders nothing even if it ever mounted.
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
      const detailText = await page.locator("body").innerText();
      expect(detailText).not.toMatch(/epic/i);
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-233"], "OSS imposes no workflow transition rules"),
  { tag: specTags(["ISS-233"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 wflow ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const states = await serverProjectStates(seed.workspaceSlug, projectId, session);
    expect(states.length).toBeGreaterThanOrEqual(2);
    const first = states[0]!;
    const second = states[1]!;
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
      await test.step("no state option is disabled and no workflow message renders", async () => {
        await driver.propertyOpenPicker("State");
        const options = await driver.pickerOptionTexts();
        expect(options.length).toBeGreaterThanOrEqual(2);
        for (const option of options) {
          expect(await driver.pickerOptionDisabled(option)).toBe(false);
        }
        await driver.pickerPressEscape();
        const detailText = await page.locator("body").innerText();
        expect(detailText).not.toMatch(/workflow/i);
      });
      await test.step("any transition round-trips through the server", async () => {
        await driver.propertyOpenPicker("State");
        await driver.pickerPick(second.name);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).state_id, {
            timeout: 25_000,
          })
          .toBe(second.id);
        await driver.propertyOpenPicker("State");
        await driver.pickerPick(first.name);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).state_id, {
            timeout: 25_000,
          })
          .toBe(first.id);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-234"], "no team-project or issue-type filter sections in OSS"),
  { tag: specTags(["ISS-234"]) },
  async ({ driver, seed, page }) => {
    // The ce stubs mount in the sub-issues filter dropdown (FilterIssueTypes
    // renders null inside an enabled section; FilterTeamProjects and the
    // applied issue-type chips have no callers at all). The project-list
    // rich-filters toggle is dead on this surface (its filter instance is
    // never created — console error, nothing opens), so it cannot serve as
    // the absence surface.
    const tag = `NF122 filtertypes ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const parent = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} parent`, session);
    const child = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} child`, session);
    await serverAddSubIssues(seed.workspaceSlug, projectId, parent.id, [child.id], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.subIssueFiltersOpen(seed.workspaceSlug, projectId, parent.id);
      const panel = await driver.subIssueFiltersPanelText();
      // Live-surface proof: the standard sections render.
      expect(panel).toMatch(/Priority/);
      expect(panel).toMatch(/State/);
      expect(panel).toMatch(/Assignee/);
      // The edition-only sections render nothing.
      expect(panel).not.toMatch(/team/i);
      expect(panel).not.toMatch(/issue.?type|work.?item.?type/i);
      const detailText = await page.locator("body").innerText();
      expect(detailText).not.toMatch(/team.?project/i);
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, child.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, parent.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-235"], "detail shows only the copyable identifier, no type UI"),
  { tag: specTags(["ISS-235"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 ident ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
      await test.step("the identifier renders bare and copies on click", async () => {
        const identifier = await driver.detailIdentifierText();
        expect(identifier).toMatch(/^[A-Z0-9]+-\d+$/);
        // Grant clipboard access before the click: the write needs the
        // permission too, and the first read is what grants it.
        await driver.rulesReadClipboard();
        await driver.detailIdentifierCopy();
        await expect.poll(() => driver.rulesReadClipboard(), { timeout: 10_000 }).toContain(identifier);
      });
      await test.step("no type switcher, templates or property widgets render", async () => {
        const detailText = await page.locator("body").innerText();
        expect(detailText).not.toMatch(/issue.?type/i);
        expect(detailText).not.toMatch(/work.?item.?type/i);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-236"], "no overdue alert or transfer-hop note renders in OSS"),
  { tag: specTags(["ISS-236"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 accents ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      { cycleView: true },
      session
    );
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      `${tag} cycle`,
      "2026-01-05",
      "2026-01-20",
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    // An overdue due date plus cycle membership: both accents would render
    // in cloud, neither may render here.
    await serverPatchIssue(
      seed.workspaceSlug,
      projectId,
      issue.id,
      { target_date: "2020-05-05", cycle_id: cycleId },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
      const dueText = await driver.propertyValueText("Due date");
      expect(dueText).toContain("May 05, 2020");
      expect(dueText).not.toMatch(/overdue/i);
      const cycleText = await driver.propertyValueText("Cycle");
      expect(cycleText).toContain("cycle");
      expect(cycleText).not.toMatch(/transfer/i);
    } finally {
      await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { cycle_id: null }, session).catch(() => {});
      await serverDeleteCycle(seed.workspaceSlug, projectId, cycleId, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-237"], "gantt renders no dependency layer in OSS"),
  { tag: specTags(["ISS-237"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 ganttdeps ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      { issueViewsView: true },
      session
    );
    const issueName = `${tag} issue`;
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, issueName, session);
    // Reach the gantt through a saved API-created view: no layout-switcher
    // driver needed and no sibling-area coupling.
    const viewId = await serverCreateView(seed.workspaceSlug, projectId, `${tag} view`, "gantt_chart", session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.viewsOpenDetail(seed.workspaceSlug, projectId, viewId);
      expect(await driver.ganttShowsIssue(issueName)).toBe(true);
      const ganttText = await page.locator("body").innerText();
      expect(ganttText).not.toMatch(/dependenc/i);
    } finally {
      await serverCleanupView(seed.workspaceSlug, projectId, viewId, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
