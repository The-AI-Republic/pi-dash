// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-255): intake list shell — the two-pane layout
// with its breadcrumb, the feature-off gate, the page title, the Open/Closed
// tabs with the pending count and URL param, auto-select-first, the empty
// states, and the per-tab default statuses. Rows: INT-001, INT-002, INT-003,
// INT-004, INT-005, INT-008, INT-012.
//
// Observed behavior notes: the tab bar is two clickable divs (no roles);
// the Open badge renders only while Open is active; the list endpoint
// defaults to pending when the status param is omitted; the Open tab sends
// pending-only while Closed sends accepted/declined/duplicate; the retired
// project-inbox address redirects onto this route. Old bug NEWFRONT-262:
// the default status filter always counts as applied, so the dedicated
// empty-Open and empty-Closed states never render (no-results shows
// instead); the INT-008 scenario pins that with the intended behavior in
// the row.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCleanupWorkspaceMember,
  serverCompleteOnboarding,
  serverCreateInboxIssue,
  serverCreateProjectWithFlags,
  serverIntakeShellInboxIssues,
  serverIntakeShellInboxView,
  serverIntakeShellPatchInboxIssue,
  serverIntakeShellSetInboxView,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  signInSessionWithRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const PENDING = -2;
const DECLINED = -1;
const SNOOZED = 0;
const ACCEPTED = 1;
const DUPLICATE = 2;

function queryOf(path: string): URLSearchParams {
  return new URL(path, "http://parity.invalid").searchParams;
}

async function removeWorkspaceMember(workspaceSlug: string, userId: string, session: string): Promise<void> {
  const membership = await serverWorkspaceMembers(workspaceSlug, session).catch(() => []);
  const mine = membership.find((row) => row.userId === userId);
  if (mine) await serverCleanupWorkspaceMember(workspaceSlug, mine.membershipId, session);
}

test(
  specTitle(["INT-001"], "intake shell renders list plus detail under the project trail, with a pick placeholder"),
  { tag: specTags(["INT-001"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF255 shell ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectName = `${tag} project`;
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectName,
      parityProjectIdentifier("N255"),
      { inboxView: true },
      session
    );
    const itemTitle = `${tag} item`;
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, itemTitle, session);
    expect(await serverIntakeShellInboxView(seed.workspaceSlug, projectId, session)).toBe(true);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("empty shell shows the tabs, the trail and the pick placeholder", async () => {
        const emptyId = await serverCreateProjectWithFlags(
          seed.workspaceSlug,
          `${tag} empty`,
          parityProjectIdentifier("N255"),
          { inboxView: true },
          session
        );
        try {
          await driver.intakeShellOpen(seed.workspaceSlug, emptyId);
          expect(await driver.intakeShellTabLabels()).toEqual(["Open", "Closed"]);
          const trail = await driver.intakeShellBreadcrumbTrail();
          expect(trail.some((crumb) => crumb.includes(`${tag} empty`))).toBe(true);
          expect(trail).toContain("Intake");
          const href = await driver.intakeShellBreadcrumbIntakeHref();
          expect(href ?? "").toContain(`/projects/${emptyId}/intake`);
          expect(await driver.intakeShellPlaceholderVisible()).toBe(true);
          expect(await driver.intakeShellFeatureGateVisible()).toBe(false);
        } finally {
          await serverCleanupProject(seed.workspaceSlug, emptyId, session);
        }
      });

      await test.step("a selected request shows the detail pane instead of the placeholder", async () => {
        await driver.intakeShellOpen(seed.workspaceSlug, projectId);
        // The shell auto-selects the first row; the detail assertions below
        // observe that (INT-005 owns the redirect row itself).
        await expect
          .poll(async () => queryOf(await driver.currentPath()).get("inboxIssueId"), { timeout: 60_000 })
          .toBe(inbox.issueId);
        expect(await driver.rulesIntakeTriageVisible()).toBe(true);
        expect(await driver.intakeShellPlaceholderVisible()).toBe(false);
        const listed = await serverIntakeShellInboxIssues(seed.workspaceSlug, projectId, session);
        expect(listed.rows.map((row) => row.name)).toContain(itemTitle);
      });

      await test.step("the retired project-inbox address lands on the intake route", async () => {
        await driver.intakeShellOpenRetiredInbox(seed.workspaceSlug, projectId);
        const path = await driver.currentPath();
        expect(path).toContain(`/projects/${projectId}/intake`);
        expect(await driver.intakeShellTabLabels()).toEqual(["Open", "Closed"]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-002"], "admins get an enabled settings shortcut on intake-off projects"),
  { tag: specTags(["INT-002"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF255 gate ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const offId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} off`,
      parityProjectIdentifier("N255"),
      {},
      session
    );
    await serverIntakeShellSetInboxView(seed.workspaceSlug, offId, session, false);
    expect(await serverIntakeShellInboxView(seed.workspaceSlug, offId, session)).toBe(false);
    const onId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} on`,
      parityProjectIdentifier("N255"),
      { inboxView: true },
      session
    );
    expect(await serverIntakeShellInboxView(seed.workspaceSlug, onId, session)).toBe(true);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the gate offers an enabled shortcut into feature settings", async () => {
        await driver.intakeShellOpen(seed.workspaceSlug, offId);
        expect(await driver.intakeShellFeatureGateVisible()).toBe(true);
        expect(await driver.intakeShellListTitles()).toEqual([]);
        expect(await driver.intakeShellFeatureGateActionEnabled()).toBe(true);
        const landed = await driver.intakeShellFeatureGateActionGo();
        expect(landed).toContain(`/settings/projects/${offId}/features`);
      });

      await test.step("intake-on projects show the list instead of the gate", async () => {
        await driver.intakeShellOpen(seed.workspaceSlug, onId);
        expect(await driver.intakeShellFeatureGateVisible()).toBe(false);
        expect(await driver.intakeShellTabLabels()).toEqual(["Open", "Closed"]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, offId, session);
      await serverCleanupProject(seed.workspaceSlug, onId, session);
    }
  }
);

test(
  specTitle(["INT-002"], "members see the gate with the settings shortcut disabled"),
  { tag: specTags(["INT-002"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF255 gate ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const offId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} off`,
      parityProjectIdentifier("N255"),
      {},
      session
    );
    await serverIntakeShellSetInboxView(seed.workspaceSlug, offId, session, false);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf255gate", session);
    await serverCompleteOnboarding(member.session);
    await serverAddProjectMembers(seed.workspaceSlug, offId, [{ memberId: member.userId, role: 15 }], session);
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.intakeShellOpen(seed.workspaceSlug, offId);
      expect(await driver.intakeShellFeatureGateVisible()).toBe(true);
      expect(await driver.intakeShellFeatureGateActionEnabled()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, offId, session);
      await removeWorkspaceMember(seed.workspaceSlug, member.userId, session);
    }
  }
);

test(
  specTitle(["INT-002"], "guests see the gate with the settings shortcut disabled"),
  { tag: specTags(["INT-002"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF255 gate ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const offId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} off`,
      parityProjectIdentifier("N255"),
      {},
      session
    );
    await serverIntakeShellSetInboxView(seed.workspaceSlug, offId, session, false);
    const guest = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf255gate", session);
    await serverCompleteOnboarding(guest.session);
    await serverAddProjectMembers(seed.workspaceSlug, offId, [{ memberId: guest.userId, role: 5 }], session);
    try {
      await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);
      await driver.intakeShellOpen(seed.workspaceSlug, offId);
      expect(await driver.intakeShellFeatureGateVisible()).toBe(true);
      expect(await driver.intakeShellFeatureGateActionEnabled()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, offId, session);
      await removeWorkspaceMember(seed.workspaceSlug, guest.userId, session);
    }
  }
);

test(
  specTitle(["INT-003"], "page title carries the project name plus the Intake label"),
  { tag: specTags(["INT-003"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF255 title ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectName = `${tag} project`;
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectName,
      parityProjectIdentifier("N255"),
      { inboxView: true },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.intakeShellOpen(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.intakeShellPageTitle(), { timeout: 60_000 }).toBe(`${projectName} - Intake`);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-004"], "Open/Closed tabs carry a live pending count, sync the URL and refetch on switch"),
  { tag: specTags(["INT-004"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF255 tabs ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N255"),
      { inboxView: true },
      session
    );
    const firstTitle = `${tag} one`;
    const secondTitle = `${tag} two`;
    const closedTitle = `${tag} closed`;
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, firstTitle, session);
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, secondTitle, session);
    const closed = await serverCreateInboxIssue(seed.workspaceSlug, projectId, closedTitle, session);
    await serverIntakeShellPatchInboxIssue(
      seed.workspaceSlug,
      projectId,
      closed.issueId,
      { status: ACCEPTED },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.intakeShellBeginListSpy();
      await driver.intakeShellOpen(seed.workspaceSlug, projectId);

      await test.step("Open shows the pending rows with their total and the tab in the URL", async () => {
        expect(await driver.intakeShellTabLabels()).toEqual(["Open", "Closed"]);
        await expect.poll(() => driver.intakeShellActiveTab(), { timeout: 30_000 }).toBe("open");
        await expect
          .poll(() => driver.intakeShellListTitles(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([firstTitle, secondTitle]));
        expect(await driver.intakeShellListTitles()).not.toContain(closedTitle);
        await expect.poll(() => driver.intakeShellOpenCount(), { timeout: 30_000 }).toBe("2");
        expect(queryOf(await driver.currentPath()).get("currentTab")).toBe("open");
        const pending = await serverIntakeShellInboxIssues(seed.workspaceSlug, projectId, session, `${PENDING}`);
        expect(pending.total).toBe(2);
      });

      const readsBefore = (await driver.intakeShellListQueries()).length;

      await test.step("Closed lists the resolved rows and drops the badge", async () => {
        await driver.intakeShellClickTab("closed");
        await expect.poll(() => driver.intakeShellActiveTab(), { timeout: 30_000 }).toBe("closed");
        await expect.poll(() => driver.intakeShellListTitles(), { timeout: 60_000 }).toEqual([closedTitle]);
        expect(await driver.intakeShellOpenCount()).toBe(null);
        expect(queryOf(await driver.currentPath()).get("currentTab")).toBe("closed");
        const queries = await driver.intakeShellListQueries();
        expect(queries.length).toBeGreaterThan(readsBefore);
        expect(queries[queries.length - 1]?.params["status"]).toBe(`${ACCEPTED},${DECLINED},${DUPLICATE}`);
      });

      await test.step("switching back refetches with the live pending total", async () => {
        const thirdTitle = `${tag} three`;
        await serverCreateInboxIssue(seed.workspaceSlug, projectId, thirdTitle, session);
        await driver.intakeShellClickTab("open");
        await expect
          .poll(() => driver.intakeShellListTitles(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([firstTitle, secondTitle, thirdTitle]));
        await expect.poll(() => driver.intakeShellOpenCount(), { timeout: 30_000 }).toBe("3");
        expect(queryOf(await driver.currentPath()).get("currentTab")).toBe("open");
        const queries = await driver.intakeShellListQueries();
        expect(queries[queries.length - 1]?.params["status"]).toBe(`${PENDING}`);
        const pending = await serverIntakeShellInboxIssues(seed.workspaceSlug, projectId, session, `${PENDING}`);
        expect(pending.total).toBe(3);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-005"], "the first row auto-selects on entry while an empty tab selects nothing"),
  { tag: specTags(["INT-005"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF255 first ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N255"),
      { inboxView: true },
      session
    );
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} alpha`, session);
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} beta`, session);
    const serverFirst = (await serverIntakeShellInboxIssues(seed.workspaceSlug, projectId, session)).rows[0];
    if (!serverFirst) throw new Error("[parity] intake fixtures did not persist.");
    const emptyId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} empty`,
      parityProjectIdentifier("N255"),
      { inboxView: true },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("a non-empty list redirects onto its first row", async () => {
        await driver.intakeShellOpen(seed.workspaceSlug, projectId);
        await expect
          .poll(async () => queryOf(await driver.currentPath()).get("inboxIssueId"), { timeout: 60_000 })
          .toBe(serverFirst.issueId);
        await expect
          .poll(() => driver.intakeShellListTitles(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([`${tag} alpha`, `${tag} beta`]));
        expect(await driver.intakeShellPlaceholderVisible()).toBe(false);
      });

      await test.step("an empty tab selects nothing", async () => {
        await driver.intakeShellOpen(seed.workspaceSlug, emptyId);
        expect(await driver.intakeShellListTitles()).toEqual([]);
        expect(queryOf(await driver.currentPath()).get("inboxIssueId")).toBe(null);
        expect(await driver.intakeShellPlaceholderVisible()).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await serverCleanupProject(seed.workspaceSlug, emptyId, session);
    }
  }
);

test(
  specTitle(
    ["INT-008"],
    "bug: NEWFRONT-262 empty Open/Closed show the no-results state; the filtered-to-nothing branch holds"
  ),
  { tag: specTags(["INT-008"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF255 empty ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N255"),
      { inboxView: true },
      session
    );
    const itemTitle = `${tag} item`;
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, itemTitle, session);
    const emptyId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} empty`,
      parityProjectIdentifier("N255"),
      { inboxView: true },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("filtering to nothing shows the no-results state", async () => {
        await driver.intakeShellBeginListSpy();
        await driver.intakeShellOpen(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.intakeShellListTitles(), { timeout: 60_000 }).toEqual([itemTitle]);
        await driver.intakeShellFilterToggleStatus("Snoozed");
        await driver.intakeShellFilterToggleStatus("Pending");
        await expect.poll(() => driver.intakeShellListEmptyHeading(), { timeout: 60_000 }).toBe("No matching results.");
        const queries = await driver.intakeShellListQueries();
        expect(queries[queries.length - 1]?.params["status"]).toBe(`${SNOOZED}`);
        const narrowed = await serverIntakeShellInboxIssues(seed.workspaceSlug, projectId, session, `${SNOOZED}`);
        expect(narrowed.rows).toEqual([]);
        await driver.intakeShellFilterToggleStatus("Pending");
        await expect.poll(() => driver.intakeShellListTitles(), { timeout: 60_000 }).toEqual([itemTitle]);
      });

      // bug: NEWFRONT-262 — the default status filter always counts as
      // applied, so the dedicated empty states never render. Intended: empty
      // Open guides with a create action, empty Closed shows its
      // informational state.
      await test.step("empty Open shows the no-results state instead of the guidance", async () => {
        await driver.intakeShellOpen(seed.workspaceSlug, emptyId);
        await expect.poll(() => driver.intakeShellListEmptyHeading(), { timeout: 60_000 }).toBe("No matching results.");
      });

      await test.step("empty Closed shows the no-results state instead of its informational state", async () => {
        await driver.intakeShellClickTab("closed");
        await expect.poll(() => driver.intakeShellListEmptyHeading(), { timeout: 60_000 }).toBe("No matching results.");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await serverCleanupProject(seed.workspaceSlug, emptyId, session);
    }
  }
);

test(
  specTitle(["INT-012"], "tabs default to their own statuses before any explicit filter"),
  { tag: specTags(["INT-012"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF255 defaults ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N255"),
      { inboxView: true },
      session
    );
    const pendingTitle = `${tag} pending`;
    const snoozedTitle = `${tag} snoozed`;
    const acceptedTitle = `${tag} accepted`;
    const declinedTitle = `${tag} declined`;
    const duplicateTitle = `${tag} duplicate`;
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, pendingTitle, session);
    const snoozed = await serverCreateInboxIssue(seed.workspaceSlug, projectId, snoozedTitle, session);
    await serverIntakeShellPatchInboxIssue(
      seed.workspaceSlug,
      projectId,
      snoozed.issueId,
      { status: SNOOZED, snoozed_till: "2030-01-01T00:00:00Z" },
      session
    );
    const accepted = await serverCreateInboxIssue(seed.workspaceSlug, projectId, acceptedTitle, session);
    await serverIntakeShellPatchInboxIssue(
      seed.workspaceSlug,
      projectId,
      accepted.issueId,
      { status: ACCEPTED },
      session
    );
    const declined = await serverCreateInboxIssue(seed.workspaceSlug, projectId, declinedTitle, session);
    await serverIntakeShellPatchInboxIssue(
      seed.workspaceSlug,
      projectId,
      declined.issueId,
      { status: DECLINED },
      session
    );
    const duplicate = await serverCreateInboxIssue(seed.workspaceSlug, projectId, duplicateTitle, session);
    await serverIntakeShellPatchInboxIssue(
      seed.workspaceSlug,
      projectId,
      duplicate.issueId,
      { status: DUPLICATE, duplicate_to: accepted.issueId },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.intakeShellBeginListSpy();
      await driver.intakeShellOpen(seed.workspaceSlug, projectId);

      await test.step("Open defaults to the pending rows", async () => {
        await expect.poll(() => driver.intakeShellListTitles(), { timeout: 60_000 }).toEqual([pendingTitle]);
        const queries = await driver.intakeShellListQueries();
        expect(queries.length).toBeGreaterThan(0);
        expect(queries[0]?.params["status"]).toBe(`${PENDING}`);
        const fallback = await serverIntakeShellInboxIssues(seed.workspaceSlug, projectId, session);
        expect(fallback.rows.map((row) => row.name)).toEqual([pendingTitle]);
      });

      await test.step("Closed defaults to the resolved rows", async () => {
        await driver.intakeShellClickTab("closed");
        await expect
          .poll(async () => [...(await driver.intakeShellListTitles())].sort(), { timeout: 60_000 })
          .toEqual([acceptedTitle, declinedTitle, duplicateTitle].sort());
        const queries = await driver.intakeShellListQueries();
        expect(queries[queries.length - 1]?.params["status"]).toBe(`${ACCEPTED},${DECLINED},${DUPLICATE}`);
        const resolved = await serverIntakeShellInboxIssues(
          seed.workspaceSlug,
          projectId,
          session,
          `${ACCEPTED},${DECLINED},${DUPLICATE}`
        );
        expect(resolved.rows.map((row) => row.name).sort()).toEqual(
          [acceptedTitle, declinedTitle, duplicateTitle].sort()
        );
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
