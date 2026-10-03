// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): permissions per role — guests read
// but cannot write anywhere, members edit issues but cannot manage
// labels or delete others' issues, and the admin creator keeps full
// control. Row: ISS-222 (permissions per role).
//
// Observed behavior notes (the inventory row carries them at update
// time): guests read issues only when the project sets
// guest_view_all_features (otherwise even GET is a 403); every refused
// write below is a server 403, and the UI gates match (no list
// quick-add, disabled sidebar pickers, a static title, not-authorized
// label settings, no Edit/Delete on others' comments; the sidebar
// create button shares the quick-add label and renders disabled for
// guests); label writes are admin-only for members and guests alike;
// issue delete is
// creator-or-admin (a member's PATCH lands but their DELETE of
// another's issue is refused). Archived read-only is proven by
// ISS-220 and cited, not redone. GitHub-synced issues disable the
// title/description editors (is_synced short-circuit in main-content)
// but need a provider+repo binding fixture no API exposes simply, so
// that clause is a row note, not a scenario.
import { test, expect } from "../fixtures";
import {
  parityApiBase,
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCleanupWorkspaceMember,
  serverComments,
  serverCompleteOnboarding,
  serverCreateIssueFull,
  serverCreateLabel,
  serverCreateProjectWithFlags,
  serverIssue,
  serverPatchProject,
  serverPostComment,
  serverProjectMembers,
  serverProvisionWorkspaceMember,
  serverRequestStatus,
  serverWorkspaceMembers,
  signInSession,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/**
 * Open the project list until `issueName` renders, reopening twice: the
 * oracle dev server stalls whole renders under shared-stack load, and a
 * stalled first open must not fail a permission assertion. A genuinely
 * absent row still fails on the final 60s poll.
 */
async function openListSettled(
  driver: ParityDriver,
  workspaceSlug: string,
  projectId: string,
  issueName: string
): Promise<void> {
  for (let round = 1; round <= 2; round++) {
    await driver.openProjectIssues(workspaceSlug, projectId);
    const settled = await expect
      .poll(async () => (await driver.visibleIssueNames()).includes(issueName), { timeout: 30_000 })
      .toBe(true)
      .then(
        () => true,
        () => false
      );
    if (settled) return;
  }
  await expect.poll(async () => (await driver.visibleIssueNames()).includes(issueName), { timeout: 60_000 }).toBe(true);
}

/** Seed guest user id, resolved live (never hardcoded across stacks). */
async function seedGuestUserId(seed: ParitySeedFacts, session: string): Promise<string> {
  const members = await serverProjectMembers(seed.workspaceSlug, seed.projectId, session);
  const guest = members.find((row) => row.role === 5);
  if (!guest) throw new Error("[parity] seed project has no role-5 guest member.");
  return guest.userId;
}

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

test(
  specTitle(["ISS-222"], "guest reads but cannot write: no quick-add, disabled sidebar, no labels, no comment tools"),
  { tag: specTags(["ISS-222"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 pmg${Date.now()}`;
    const guestCreds = requireGuest(seed);
    const session = await signInSession(seed.email, seed.password);
    const guestSession = await signInSessionRetry(guestCreds.email, guestCreds.password);
    const apiBase = parityApiBase();
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    await serverPatchProject(seed.workspaceSlug, projectId, session, { guest_view_all_features: true });
    await serverAddProjectMembers(
      seed.workspaceSlug,
      projectId,
      [{ memberId: await seedGuestUserId(seed, session), role: 5 }],
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const comment = await serverPostComment(
      seed.workspaceSlug,
      projectId,
      issue.id,
      `<p>${tag} owner comment</p>`,
      session
    );
    const issueUrl = `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${issue.id}/`;
    try {
      await driver.rulesEnsureSignedIn(guestCreds.email, guestCreds.password, seed.workspaceSlug);

      await test.step("the project list offers no quick-add to guests", async () => {
        // The sidebar create button shares the trigger's label but is a
        // different control (disabled for guests); the driver excludes it.
        await openListSettled(driver, seed.workspaceSlug, projectId, `${tag} issue`);
        expect(await driver.projectQuickAddVisible()).toBe(false);
      });

      await test.step("detail sidebar pickers are disabled and the title is static", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        expect(await driver.propertyPickerDisabled("Priority")).toBe(true);
        expect(await driver.issueTitleInputEnabled()).toBe(false);
      });

      await test.step("another author's comment offers no Edit/Delete to guests", async () => {
        // Same detail open as the sidebar step above: reopening only
        // invites dev-server stalls, and the feed is already rendered.
        const options = await driver.rulesCommentMenuOptions(comment.id);
        expect(options).not.toContain("edit");
        expect(options).not.toContain("delete");
      });

      await test.step("label settings refuse guests", async () => {
        await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.settingsLabelsAccessDenied(), { timeout: 30_000 }).toBe(true);
      });

      await test.step("the server refuses guest writes and the rows survive", async () => {
        expect(
          (
            await serverRequestStatus(
              "POST",
              `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/`,
              guestSession,
              { name: `${tag} guest attempt` }
            )
          ).status
        ).toBe(403);
        expect((await serverRequestStatus("PATCH", issueUrl, guestSession, { priority: "urgent" })).status).toBe(403);
        expect((await serverRequestStatus("DELETE", issueUrl, guestSession)).status).toBe(403);
        expect(
          (
            await serverRequestStatus(
              "POST",
              `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issue-labels/`,
              guestSession,
              { name: `${tag} gx`, color: "#ff0000" }
            )
          ).status
        ).toBe(403);
        expect(
          (
            await serverRequestStatus(
              "PATCH",
              `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${issue.id}/comments/${comment.id}/`,
              guestSession,
              { comment_html: "<p>gx</p>" }
            )
          ).status
        ).toBe(403);
        expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).name).toBe(`${tag} issue`);
        expect(
          (await serverComments(seed.workspaceSlug, projectId, issue.id, session)).some((row) => row.id === comment.id)
        ).toBe(true);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-222"], "member edits issues but cannot manage labels or delete others' issues"),
  { tag: specTags(["ISS-222"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 pmm${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const apiBase = parityApiBase();
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf122pmm", session);
    await serverCompleteOnboarding(member.session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.userId, role: 15 }], session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const comment = await serverPostComment(
      seed.workspaceSlug,
      projectId,
      issue.id,
      `<p>${tag} owner comment</p>`,
      session
    );
    await serverCreateLabel(seed.workspaceSlug, projectId, `${tag} label`, "#00d084", session);
    const issueUrl = `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${issue.id}/`;
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);

      await test.step("members keep the list quick-add and an editable title", async () => {
        await openListSettled(driver, seed.workspaceSlug, projectId, `${tag} issue`);
        expect(await driver.projectQuickAddVisible()).toBe(true);
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        expect(await driver.issueTitleInputEnabled()).toBe(true);
      });

      await test.step("members see the label list but no Add control", async () => {
        await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsLabelsAccessDenied()).toBe(false);
        await expect.poll(() => driver.settingsLabelsNames(), { timeout: 30_000 }).toContain(`${tag} label`);
        expect(await driver.settingsLabelsAddVisible()).toBe(false);
      });

      await test.step("the server lets members edit but refuses labels, deletes and others' comments", async () => {
        const patch = await serverRequestStatus("PATCH", issueUrl, member.session, { priority: "urgent" });
        expect(patch.status).toBeGreaterThanOrEqual(200);
        expect(patch.status).toBeLessThan(300);
        expect((await serverRequestStatus("DELETE", issueUrl, member.session)).status).toBe(403);
        expect(
          (
            await serverRequestStatus(
              "POST",
              `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issue-labels/`,
              member.session,
              { name: `${tag} mx`, color: "#ff0000" }
            )
          ).status
        ).toBe(403);
        expect(
          (
            await serverRequestStatus(
              "PATCH",
              `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${issue.id}/comments/${comment.id}/`,
              member.session,
              { comment_html: "<p>mx</p>" }
            )
          ).status
        ).toBe(403);
        expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).priority).toBe("urgent");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      const membership = (await serverWorkspaceMembers(seed.workspaceSlug, session).catch(() => [])) as {
        membershipId: string;
        userId: string;
      }[];
      const mine = membership.find((row) => row.userId === member.userId);
      if (mine) await serverCleanupWorkspaceMember(seed.workspaceSlug, mine.membershipId, session);
    }
  }
);

test(
  specTitle(["ISS-222"], "admin creator keeps quick-add, editing, labels, comment tools and delete"),
  { tag: specTags(["ISS-222"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 pmo${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const apiBase = parityApiBase();
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const doomed = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} doomed`, session);
    const comment = await serverPostComment(
      seed.workspaceSlug,
      projectId,
      issue.id,
      `<p>${tag} owner comment</p>`,
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("quick-add, editable title, comment tools and label creation stay available", async () => {
        await openListSettled(driver, seed.workspaceSlug, projectId, `${tag} issue`);
        expect(await driver.projectQuickAddVisible()).toBe(true);
        await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
        expect(await driver.issueTitleInputEnabled()).toBe(true);
        const options = await driver.rulesCommentMenuOptions(comment.id);
        expect(options).toContain("edit");
        expect(options).toContain("delete");
        await driver.settingsLabelsOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsLabelsAccessDenied()).toBe(false);
        expect(await driver.settingsLabelsAddVisible()).toBe(true);
      });

      await test.step("the creator's delete lands", async () => {
        const res = await serverRequestStatus(
          "DELETE",
          `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${doomed.id}/`,
          session
        );
        expect(res.status).toBeGreaterThanOrEqual(200);
        expect(res.status).toBeLessThan(300);
        await expect
          .poll(
            async () =>
              (
                await serverRequestStatus(
                  "GET",
                  `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${doomed.id}/`,
                  session
                )
              ).status,
            { timeout: 30_000 }
          )
          .toBe(404);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, doomed.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
