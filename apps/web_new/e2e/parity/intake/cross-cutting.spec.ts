// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-260): intake cross-cutting — duplicate
// detection is absent in this checkout (cloud-only), the narrow-viewport
// layout collapses the list behind a toggle with the actions in a mobile
// menu, there is no live push, guests see only their own requests unless the
// project lets them see all, and the OSS run shows no source indicator.
// Rows: INT-018, INT-030, INT-031, INT-032, INT-034.
//
// Observed behavior notes: the detail fetch is a one-shot read that ignores
// window focus; the list refetches on tab switches and navigation only; the
// de-dupe affordances and the source pill are stubs that render nothing; the
// mobile chrome appears below the wide breakpoint with the list as a toggled
// overlay; guest list/retrieve scope to the guest's own rows unless the
// project's guest-view-all flag is set (retrieve refuses with 403). The cloud
// comparison view, source pills, and forms/email ingestion are untestable in
// this checkout and recorded as gaps in the rows.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCleanupWorkspaceMember,
  serverCompleteOnboarding,
  serverCreateInboxIssue,
  serverCreateProjectWithFlags,
  serverIntakeXPatchInboxIssue,
  serverIntakeXPatchProject,
  serverIntakeXInboxIssues,
  serverIntakeXProjectFlags,
  serverIntakeXReadInboxIssue,
  serverMe,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  signInSessionWithRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

function queryOf(path: string): URLSearchParams {
  return new URL(path, "http://parity.invalid").searchParams;
}

function intakeReads(urls: { method: string; url: string }[]): string[] {
  return urls
    .filter(({ method, url }) => method === "GET" && /\/(inbox|intake)-issues\//.test(url))
    .map(({ url }) => url);
}

async function removeWorkspaceMember(workspaceSlug: string, userId: string, session: string): Promise<void> {
  const membership = await serverWorkspaceMembers(workspaceSlug, session).catch(() => []);
  const mine = membership.find((row) => row.userId === userId);
  if (mine) await serverCleanupWorkspaceMember(workspaceSlug, mine.membershipId, session);
}

test(
  specTitle(["INT-018"], "no duplicate-detection UI while composing or viewing, and no duplicate-search traffic"),
  { tag: specTags(["INT-018"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF260 dupe ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N260"),
      { inboxView: true },
      session
    );
    const itemTitle = `${tag} password reset`;
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, itemTitle, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.intakeXBeginApiSpy();
      await driver.intakeXOpen(seed.workspaceSlug, projectId);

      await test.step("composing a similar request surfaces no duplicate UI", async () => {
        await driver.intakeXOpenCreate();
        await driver.intakeXCreateFillTitle(`${tag} password reset again`);
        // Any debounced lookup would have fired by now; the stubs issue none.
        await new Promise((resolve) => setTimeout(resolve, 2500));
        expect(await driver.intakeXCreateDuplicateDetectionVisible()).toBe(false);
      });

      await test.step("viewing a request surfaces no duplicate UI either", async () => {
        await driver.intakeXOpenDetail(seed.workspaceSlug, projectId, inbox.issueId);
        await expect.poll(() => driver.intakeXDetailTitle(), { timeout: 60_000 }).toBe(itemTitle);
        expect(await driver.intakeXDetailDuplicateDetectionVisible()).toBe(false);
      });

      await test.step("no duplicate-search traffic left the browser", async () => {
        const searches = (await driver.intakeXApiRequests()).filter(({ url }) => /duplic|dedupe|similar/i.test(url));
        expect(searches).toEqual([]);
        const stored = await serverIntakeXInboxIssues(seed.workspaceSlug, projectId, session);
        expect(stored.rows.map((row) => row.name)).toContain(itemTitle);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-030"], "narrow viewports collapse the list behind a toggle and keep the actions in a mobile menu"),
  { tag: specTags(["INT-030"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF260 narrow ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N260"),
      { inboxView: true },
      session
    );
    const firstTitle = `${tag} one`;
    const secondTitle = `${tag} two`;
    const first = await serverCreateInboxIssue(seed.workspaceSlug, projectId, firstTitle, session);
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, secondTitle, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.intakeXOpenDetail(seed.workspaceSlug, projectId, first.issueId);
      await expect.poll(() => driver.intakeXDetailTitle(), { timeout: 60_000 }).toBe(firstTitle);

      await test.step("a narrow viewport renders the mobile chrome with the list open", async () => {
        await driver.intakeXSetViewport(390, 844);
        await expect.poll(() => driver.intakeXMobileHeaderVisible(), { timeout: 30_000 }).toBe(true);
        await expect.poll(() => driver.intakeXListPaneVisible(), { timeout: 30_000 }).toBe(true);
      });

      await test.step("the toggle hides and reopens the list", async () => {
        await driver.intakeXToggleMobileSidebar();
        await expect.poll(() => driver.intakeXListPaneVisible(), { timeout: 30_000 }).toBe(false);
        await driver.intakeXToggleMobileSidebar();
        await expect.poll(() => driver.intakeXListPaneVisible(), { timeout: 30_000 }).toBe(true);
        await expect
          .poll(() => driver.intakeXListTitles(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([firstTitle, secondTitle]));
      });

      await test.step("the mobile menu keeps every triage action reachable", async () => {
        const items = await driver.intakeXMobileMenuItems();
        expect(items).toEqual(expect.arrayContaining(["Snooze", "Mark as duplicate", "Accept", "Decline", "Delete"]));
        const stored = await serverIntakeXInboxIssues(seed.workspaceSlug, projectId, session);
        expect(stored.rows.map((row) => row.name).sort()).toEqual([firstTitle, secondTitle].sort());
      });
    } finally {
      await driver.intakeXSetViewport(1440, 900).catch(() => undefined);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-031"], "outside changes surface only on refetch triggers, never spontaneously or on focus"),
  { tag: specTags(["INT-031"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF260 live ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N260"),
      { inboxView: true },
      session
    );
    const beforeTitle = `${tag} before`;
    const afterTitle = `${tag} after`;
    const addedTitle = `${tag} added`;
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, beforeTitle, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.intakeXOpenDetail(seed.workspaceSlug, projectId, inbox.issueId);
      await expect.poll(() => driver.intakeXDetailTitle(), { timeout: 60_000 }).toBe(beforeTitle);
      await driver.intakeXBeginApiSpy();

      await test.step("outside writes change the server but not the screen", async () => {
        await serverCreateInboxIssue(seed.workspaceSlug, projectId, addedTitle, session);
        await serverIntakeXPatchInboxIssue(seed.workspaceSlug, projectId, inbox.issueId, { name: afterTitle }, session);
        const stored = await serverIntakeXInboxIssues(seed.workspaceSlug, projectId, session);
        expect(stored.rows.map((row) => row.name).sort()).toEqual([afterTitle, addedTitle].sort());
        // Longer than any polling interval the screen would use; the reads
        // below prove nothing refetched.
        await new Promise((resolve) => setTimeout(resolve, 3000));
        expect(await driver.intakeXListTitles()).toEqual([beforeTitle]);
        expect(await driver.intakeXDetailTitle()).toBe(beforeTitle);
        expect(intakeReads(await driver.intakeXApiRequests())).toEqual([]);
      });

      await test.step("returning focus to the page refetches nothing", async () => {
        await driver.intakeXRefocusPage();
        expect(await driver.intakeXDetailTitle()).toBe(beforeTitle);
        expect(intakeReads(await driver.intakeXApiRequests())).toEqual([]);
      });

      await test.step("a tab switch surfaces the new row", async () => {
        await driver.intakeXClickTab("closed");
        await driver.intakeXClickTab("open");
        await expect
          .poll(() => driver.intakeXListTitles(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([addedTitle]));
        expect(intakeReads(await driver.intakeXApiRequests()).length).toBeGreaterThan(0);
      });

      await test.step("explicit navigation surfaces the renamed detail", async () => {
        await driver.intakeXOpenDetail(seed.workspaceSlug, projectId, inbox.issueId);
        await expect.poll(() => driver.intakeXDetailTitle(), { timeout: 60_000 }).toBe(afterTitle);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-032"], "a guest sees only their own requests and the detail fetch refuses the rest"),
  { tag: specTags(["INT-032"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    if (!seed.guestEmail || !seed.guestPassword) throw new Error("[parity] seed carries no guest identity.");
    const tag = `NF260 scoped ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const guestSession = await signInSessionWithRetry(seed.guestEmail, seed.guestPassword);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N260"),
      { inboxView: true },
      session
    );
    const guestUserId = (await serverMe(guestSession)).id;
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: guestUserId, role: 5 }], session);
    const mineTitle = `${tag} mine`;
    const theirsTitle = `${tag} theirs`;
    const mine = await serverCreateInboxIssue(seed.workspaceSlug, projectId, mineTitle, guestSession);
    const theirs = await serverCreateInboxIssue(seed.workspaceSlug, projectId, theirsTitle, session);
    expect(await serverIntakeXProjectFlags(seed.workspaceSlug, projectId, session)).toEqual({
      inboxView: true,
      guestViewAll: false,
    });
    try {
      await test.step("the guest list holds only the guest's own row", async () => {
        await driver.rulesEnsureSignedIn(seed.guestEmail, seed.guestPassword, seed.workspaceSlug);
        await driver.intakeXOpen(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.intakeXListTitles(), { timeout: 60_000 }).toEqual([mineTitle]);
        const listed = await serverIntakeXInboxIssues(seed.workspaceSlug, projectId, guestSession);
        expect(listed.rows.map((row) => row.name)).toEqual([mineTitle]);
      });

      await test.step("the detail fetch answers 200 for the own row and 403 for the other", async () => {
        expect(
          (await serverIntakeXReadInboxIssue(seed.workspaceSlug, projectId, mine.issueId, guestSession)).httpStatus
        ).toBe(200);
        expect(
          (await serverIntakeXReadInboxIssue(seed.workspaceSlug, projectId, theirs.issueId, guestSession)).httpStatus
        ).toBe(403);
      });

      await test.step("opening the other's deep link falls back to the scoped list", async () => {
        await driver.intakeXOpenDetail(seed.workspaceSlug, projectId, theirs.issueId);
        await expect.poll(() => driver.intakeXListTitles(), { timeout: 60_000 }).toEqual([mineTitle]);
        expect(queryOf(await driver.currentPath()).get("inboxIssueId")).not.toBe(theirs.issueId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-032"], "a guest sees every request once the project lets guests view all"),
  { tag: specTags(["INT-032"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    if (!seed.guestEmail || !seed.guestPassword) throw new Error("[parity] seed carries no guest identity.");
    const tag = `NF260 flag ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const guestSession = await signInSessionWithRetry(seed.guestEmail, seed.guestPassword);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N260"),
      { inboxView: true },
      session
    );
    const guestUserId = (await serverMe(guestSession)).id;
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: guestUserId, role: 5 }], session);
    const mineTitle = `${tag} mine`;
    const theirsTitle = `${tag} theirs`;
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, mineTitle, guestSession);
    const theirs = await serverCreateInboxIssue(seed.workspaceSlug, projectId, theirsTitle, session);
    await serverIntakeXPatchProject(seed.workspaceSlug, projectId, { guest_view_all_features: true }, session);
    expect(await serverIntakeXProjectFlags(seed.workspaceSlug, projectId, session)).toEqual({
      inboxView: true,
      guestViewAll: true,
    });
    try {
      await driver.rulesEnsureSignedIn(seed.guestEmail, seed.guestPassword, seed.workspaceSlug);
      await driver.intakeXOpen(seed.workspaceSlug, projectId);
      await expect
        .poll(() => driver.intakeXListTitles(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([mineTitle, theirsTitle]));
      expect(
        (await serverIntakeXReadInboxIssue(seed.workspaceSlug, projectId, theirs.issueId, guestSession)).httpStatus
      ).toBe(200);
      const listed = await serverIntakeXInboxIssues(seed.workspaceSlug, projectId, guestSession);
      expect(listed.rows.map((row) => row.name).sort()).toEqual([mineTitle, theirsTitle].sort());
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-032"], "a project member always sees every request"),
  { tag: specTags(["INT-032"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF260 member ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N260"),
      { inboxView: true },
      session
    );
    const firstTitle = `${tag} one`;
    const secondTitle = `${tag} two`;
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, firstTitle, session);
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, secondTitle, session);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf260member", session);
    await serverCompleteOnboarding(member.session);
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.userId, role: 15 }], session);
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.intakeXOpen(seed.workspaceSlug, projectId);
      await expect
        .poll(() => driver.intakeXListTitles(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([firstTitle, secondTitle]));
      const listed = await serverIntakeXInboxIssues(seed.workspaceSlug, projectId, member.session);
      expect(listed.rows.map((row) => row.name).sort()).toEqual([firstTitle, secondTitle].sort());
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await removeWorkspaceMember(seed.workspaceSlug, member.userId, session);
    }
  }
);

test(
  specTitle(["INT-032"], "a project admin always sees every request"),
  { tag: specTags(["INT-032"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF260 admin ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N260"),
      { inboxView: true },
      session
    );
    const firstTitle = `${tag} one`;
    const secondTitle = `${tag} two`;
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, firstTitle, session);
    await serverCreateInboxIssue(seed.workspaceSlug, projectId, secondTitle, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.intakeXOpen(seed.workspaceSlug, projectId);
      await expect
        .poll(() => driver.intakeXListTitles(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([firstTitle, secondTitle]));
      const listed = await serverIntakeXInboxIssues(seed.workspaceSlug, projectId, session);
      expect(listed.rows.map((row) => row.name).sort()).toEqual([firstTitle, secondTitle].sort());
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["INT-034"], "the OSS run shows no source indicator and no duplicate UI"),
  { tag: specTags(["INT-034"]) },
  async ({ driver, seed }) => {
    test.setTimeout(420_000);
    const tag = `NF260 oss ${Date.now()}`;
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N260"),
      { inboxView: true },
      session
    );
    const itemTitle = `${tag} item`;
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, itemTitle, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.intakeXOpen(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.intakeXListTitles(), { timeout: 60_000 }).toEqual([itemTitle]);

      await test.step("rows carry sources server-side but render no indicator", async () => {
        expect(await driver.intakeXSourceIndicatorVisible()).toBe(false);
        const stored = await serverIntakeXInboxIssues(seed.workspaceSlug, projectId, session);
        expect(stored.rows.map((row) => row.source)).toEqual(["IN_APP"]);
      });

      await test.step("the detail renders no duplicate UI", async () => {
        await driver.intakeXOpenDetail(seed.workspaceSlug, projectId, inbox.issueId);
        await expect.poll(() => driver.intakeXDetailTitle(), { timeout: 60_000 }).toBe(itemTitle);
        expect(await driver.intakeXDetailDuplicateDetectionVisible()).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
