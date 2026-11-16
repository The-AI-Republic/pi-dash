// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-411): workspace members + invitations.
// Rows WSET-008 (roster count, search, role filter, active-first sort),
// WSET-009 (bulk invite with per-invitee roles), WSET-010
// (pending-invitations group), WSET-011 (row identity + activity entry),
// WSET-012 (role change), WSET-013 (remove / self-leave), WSET-014
// (loading + no-match states), WSET-015 (billing upsell entry).
//
// Observed behavior notes (pinned by these scenarios; the inventory rows
// carry them at update time): the roster count chip excludes bot
// memberships; search matches first, last and display names plus email,
// while pending invitations match by email; the role filter offers
// Admin/Member/Guest/Suspended and combines with the search; removing a
// member (or leaving) deactivates server-side and the row persists as a
// Suspended row sorted last across reloads; the Add control and the
// pending group are admin-only (members see neither); role options list
// all three roles for the admin actor; the server refuses self role
// changes (400) and member-actor member PATCHes (403) while writes to
// deactivated rows 404; self-removal leaves through the leave endpoint
// and lands on a not-authorized gate for the left workspace; no
// skeleton renders while the roster loads (bug NEWFRONT-422: the loader
// guard can never pass); failed writes toast and revert or keep their
// rows; the activity and billing entries beside the Add control render
// nothing in this OSS checkout (the cloud overlay is unobservable
// here).
import { test, expect } from "../fixtures";
import {
  parityApiBase,
  serverCleanupWorkspaceInvitation,
  serverCompleteOnboarding,
  serverCreateWorkspaceInvitations,
  serverHardDeleteWorkspaceMember,
  serverPatchWorkspaceMemberRole,
  serverProvisionWorkspaceMember,
  serverRequestStatus,
  serverWorkspaceInvitations,
  serverWorkspaceMembersFull,
  signInSession,
  uniqueEmail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import { openWorkspacePage, provisionMember as provisionEditionMember, seededSession } from "./edition-session";

/** Numeric workspace roles as the server stores them. */
const ROLE = { ADMIN: 20, MEMBER: 15, GUEST: 5 } as const;

/** UI label per numeric role. */
const ROLE_LABEL: Record<number, string> = { 20: "Admin", 15: "Member", 5: "Guest" };

/** Seeded member identity (workspace member, role 15); throws when the seed lacks it. */
function requireMember(seed: ParitySeedFacts): { email: string; password: string; id: string } {
  const member = seed.mentionMember;
  if (!member) throw new Error("[parity] seed has no mentionMember; rerun parity-up.sh from a clean checkout.");
  return { email: member.email, password: member.password, id: member.id };
}

/** Seeded guest identity (workspace guest, role 5); throws when the seed lacks it. */
function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

interface ProvisionedMember {
  userId: string;
  membershipId: string;
  session: string;
  email: string;
  password: string;
  displayName: string;
}

/** Provision a workspace member at `role` with onboarding completed, for UI sign-in. */
async function provisionMember(
  seed: ParitySeedFacts,
  ownerSession: string,
  prefix: string,
  role: number = ROLE.MEMBER
): Promise<ProvisionedMember> {
  const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, prefix, ownerSession);
  let rows = await serverWorkspaceMembersFull(seed.workspaceSlug, ownerSession);
  let row = rows.find((entry) => entry.userId === member.userId);
  if (!row) throw new Error("[parity] provisioned member missing from workspace list.");
  if (role !== ROLE.MEMBER) {
    await serverPatchWorkspaceMemberRole(seed.workspaceSlug, row.membershipId, role, ownerSession);
    rows = await serverWorkspaceMembersFull(seed.workspaceSlug, ownerSession);
    row = rows.find((entry) => entry.userId === member.userId);
    if (!row) throw new Error("[parity] provisioned member missing from workspace list.");
  }
  await serverCompleteOnboarding(member.session);
  return {
    userId: member.userId,
    membershipId: row.membershipId,
    session: member.session,
    email: member.email,
    password: member.password,
    displayName: row.displayName,
  };
}

/**
 * Switch the signed-in UI user. Sign-in is a no-op while a session
 * lives, so the current user signs out first; the palette path is used
 * because settings pages render no account-menu sidebar.
 */
async function switchUser(driver: ParityDriver, email: string, password: string, workspaceSlug: string): Promise<void> {
  await driver.signOutViaCommandPalette();
  await driver.rulesEnsureSignedIn(email, password, workspaceSlug);
}

/** Latest toast message, or empty while no toast shows (for expect.poll). */
async function toastMessage(driver: ParityDriver): Promise<string> {
  return (await driver.rulesLastToast())?.message ?? "";
}

/** Latest toast title, or empty while no toast shows (for expect.poll). */
async function toastTitle(driver: ParityDriver): Promise<string> {
  return (await driver.rulesLastToast())?.title ?? "";
}

/**
 * Withdraw every pending invitation whose email starts with `prefix`.
 * Id-based cleanup misses rows when a test fails before recording ids;
 * sweeping by the test's unique prefix keeps later tests isolated.
 */
async function sweepInvitationsByPrefix(seed: ParitySeedFacts, session: string, prefix: string): Promise<void> {
  const server = await serverWorkspaceInvitations(seed.workspaceSlug, session);
  for (const row of server.filter((entry) => entry.email.startsWith(prefix))) {
    await serverCleanupWorkspaceInvitation(seed.workspaceSlug, row.id, session);
  }
}

/**
 * Open the members page as an admin and wait for the permission-gated
 * chrome (the Add control proves the self-membership read landed).
 * Roster rows render before permissions resolve, so admin-only reads
 * must wait on this gate instead of the rows.
 */
async function adminOpen(driver: ParityDriver, workspaceSlug: string): Promise<void> {
  await driver.wsSettingsMembersOpen(workspaceSlug);
  await expect.poll(() => driver.wsSettingsMembersAddVisible(), { timeout: 30_000 }).toBe(true);
}

/**
 * Suspended rows form the tail of the roster: once the first suspended
 * row appears, no active row may follow it.
 */
function expectSuspendedRowsLast(order: string[], suspended: Set<string>): void {
  const firstSuspended = order.findIndex((email) => suspended.has(email));
  if (firstSuspended === -1) return;
  expect(order.slice(firstSuspended).every((email) => suspended.has(email))).toBe(true);
}

test(
  specTitle(["WSET-008"], "roster counts, searches, filters by role, and sorts suspended rows last"),
  { tag: specTags(["WSET-008"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const guestExtra = await provisionMember(seed, session, "nf411-w8g", ROLE.GUEST);
    const memberExtra = await provisionMember(seed, session, "nf411-w8m", ROLE.MEMBER);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await adminOpen(driver, seed.workspaceSlug);

      await test.step("count chip matches the server roster without bots", async () => {
        const server = (await serverWorkspaceMembersFull(seed.workspaceSlug, session)).filter((row) => !row.isBot);
        expect(await driver.wsSettingsMembersCountChip()).toBe(server.length);
        expect(new Set(await driver.wsSettingsMembersVisibleEmails())).toEqual(new Set(server.map((row) => row.email)));
        const uiRoles = await driver.wsSettingsMembersVisibleRoles();
        for (const row of server.filter((entry) => entry.isActive)) {
          expect(uiRoles.find((ui) => ui.email === row.email)?.role).toBe(ROLE_LABEL[row.role]);
        }
      });

      await test.step("search filters by name and by email", async () => {
        await driver.wsSettingsMembersSearch("Parity Mention");
        expect(await driver.wsSettingsMembersVisibleEmails()).toEqual(["parity-mention@example.com"]);
        await driver.wsSettingsMembersSearch(guestExtra.email);
        expect(await driver.wsSettingsMembersVisibleEmails()).toEqual([guestExtra.email]);
        await driver.wsSettingsMembersSearch("");
        const server = (await serverWorkspaceMembersFull(seed.workspaceSlug, session)).filter((row) => !row.isBot);
        expect(new Set(await driver.wsSettingsMembersVisibleEmails())).toEqual(new Set(server.map((row) => row.email)));
      });

      await test.step("role filter offers four options and combines with search", async () => {
        await driver.wsSettingsMembersOpenFilters();
        expect(await driver.wsSettingsMembersFilterOptions()).toEqual(["Admin", "Member", "Guest", "Suspended"]);
        await driver.wsSettingsMembersToggleFilter("Guest");
        expect(new Set(await driver.wsSettingsMembersVisibleEmails())).toEqual(
          new Set(["parity-guest@example.com", guestExtra.email])
        );
        expect(await driver.wsSettingsMembersFilterBadge()).toBe(true);
        await driver.wsSettingsMembersCloseFilters();
        await driver.wsSettingsMembersSearch("Parity Guest");
        expect(await driver.wsSettingsMembersVisibleEmails()).toEqual(["parity-guest@example.com"]);
        await driver.wsSettingsMembersSearch("");
        await driver.wsSettingsMembersOpenFilters();
        await driver.wsSettingsMembersToggleFilter("Guest");
        expect(await driver.wsSettingsMembersFilterBadge()).toBe(false);
        await driver.wsSettingsMembersCloseFilters();
        const server = (await serverWorkspaceMembersFull(seed.workspaceSlug, session)).filter((row) => !row.isBot);
        expect(new Set(await driver.wsSettingsMembersVisibleEmails())).toEqual(new Set(server.map((row) => row.email)));
      });

      await test.step("removed members persist as suspended rows sorted last", async () => {
        // Other specs in this shared scratch workspace leave their own
        // suspended members behind, so the roster is not empty here.
        // Assert relatively: the removed member joins the suspended set
        // and every suspended row sits after every active row.
        const suspendedBefore = new Set(await driver.wsSettingsMembersSuspendedEmails());
        expect(suspendedBefore.has(memberExtra.email)).toBe(false);
        await driver.wsSettingsMemberOpenRemove(memberExtra.email);
        expect(await driver.wsSettingsRemoveDialogTitle()).toBe(`Remove ${memberExtra.displayName}?`);
        await driver.wsSettingsRemoveDialogCancel();
        expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
        expect(new Set(await driver.wsSettingsMembersSuspendedEmails())).toEqual(suspendedBefore);
        await driver.wsSettingsMemberOpenRemove(memberExtra.email);
        await driver.wsSettingsRemoveDialogConfirm();
        expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
        expect(new Set(await driver.wsSettingsMembersSuspendedEmails())).toEqual(
          new Set([...suspendedBefore, memberExtra.email])
        );
        const order = await driver.wsSettingsMembersVisibleEmails();
        expectSuspendedRowsLast(order, new Set([...suspendedBefore, memberExtra.email]));
        // Removal deactivates server-side; the row is not deleted.
        const server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
        expect(server.find((row) => row.email === memberExtra.email)?.isActive).toBe(false);
        await driver.wsSettingsMembersOpenFilters();
        await driver.wsSettingsMembersToggleFilter("Suspended");
        expect(new Set(await driver.wsSettingsMembersVisibleEmails())).toEqual(
          new Set([...suspendedBefore, memberExtra.email])
        );
        await driver.wsSettingsMembersToggleFilter("Suspended");
        await driver.wsSettingsMembersCloseFilters();
        await driver.wsSettingsMembersOpen(seed.workspaceSlug);
        expect(new Set(await driver.wsSettingsMembersSuspendedEmails())).toEqual(
          new Set([...suspendedBefore, memberExtra.email])
        );
        const reloaded = await driver.wsSettingsMembersVisibleEmails();
        expectSuspendedRowsLast(reloaded, new Set([...suspendedBefore, memberExtra.email]));
      });
    } finally {
      await serverHardDeleteWorkspaceMember(guestExtra.membershipId);
      await serverHardDeleteWorkspaceMember(memberExtra.membershipId);
    }
  }
);

test(
  specTitle(["WSET-009"], "bulk invite sends per-invitee roles; invalid and failing rows keep the dialog"),
  { tag: specTags(["WSET-009"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const created: string[] = [];
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await adminOpen(driver, seed.workspaceSlug);

      await test.step("dialog opens with one member-defaulted row offering every role", async () => {
        await driver.wsSettingsInviteOpen();
        expect(await driver.wsSettingsInviteRowCount()).toBe(1);
        expect(await driver.wsSettingsInviteRowRole(0)).toBe("Member");
        expect(await driver.wsSettingsInviteRowRoles(0)).toEqual(["Guest", "Member", "Admin"]);
      });

      await test.step("rows append and drop", async () => {
        await driver.wsSettingsInviteAddRow();
        expect(await driver.wsSettingsInviteRowCount()).toBe(2);
        await driver.wsSettingsInviteRemoveRow(1);
        expect(await driver.wsSettingsInviteRowCount()).toBe(1);
      });

      await test.step("invalid emails block submit inline with input intact", async () => {
        await driver.wsSettingsInviteFillRow(0, "not-an-email");
        await driver.wsSettingsInviteSubmit();
        expect(await driver.wsSettingsInviteRowError(0)).toContain("invalid");
        expect(await driver.wsSettingsInviteVisible()).toBe(true);
        expect(await driver.wsSettingsInviteRowValue(0)).toBe("not-an-email");
        const server = await serverWorkspaceInvitations(seed.workspaceSlug, session);
        expect(server.some((row) => row.email === "not-an-email")).toBe(false);
      });

      await test.step("inviting an existing member fails with the dialog kept", async () => {
        await driver.wsSettingsInviteFillRow(0, "parity-mention@example.com");
        await driver.wsSettingsInviteSubmit();
        expect(await driver.wsSettingsInviteVisible()).toBe(true);
        expect(await driver.wsSettingsInviteRowValue(0)).toBe("parity-mention@example.com");
        // The server reports the member conflict (400), but the UI toasts
        // its generic failure notice rather than the reason.
        await expect.poll(() => toastTitle(driver), { timeout: 30_000 }).toBe("Error!");
        const server = await serverWorkspaceInvitations(seed.workspaceSlug, session);
        expect(server.some((row) => row.email === "parity-mention@example.com")).toBe(false);
      });

      await test.step("two rows with distinct roles send and appear as pending", async () => {
        const emailA = uniqueEmail("nf411-inv-a");
        const emailB = uniqueEmail("nf411-inv-b");
        // The shared scratch workspace can carry other runs' pending
        // invitations, so the group is asserted relative to what shows
        // before this submit.
        const invitesBefore = new Set(await driver.wsSettingsInvitesVisibleEmails());
        await driver.wsSettingsInviteFillRow(0, emailA);
        await driver.wsSettingsInviteSetRowRole(0, "Guest");
        await driver.wsSettingsInviteAddRow();
        await driver.wsSettingsInviteFillRow(1, emailB);
        await driver.wsSettingsInviteSetRowRole(1, "Admin");
        await driver.wsSettingsInviteSubmit();
        expect(await driver.wsSettingsInviteVisible()).toBe(false);
        await expect.poll(() => toastMessage(driver), { timeout: 30_000 }).toContain("Invitations sent");
        expect(await driver.wsSettingsInvitesGroupVisible()).toBe(true);
        await expect
          .poll(() => driver.wsSettingsInvitesVisibleEmails(), { timeout: 30_000 })
          .toEqual(expect.arrayContaining([emailA, emailB]));
        expect(await driver.wsSettingsInvitesCount()).toBe(invitesBefore.size + 2);
        expect(new Set(await driver.wsSettingsInvitesVisibleEmails())).toEqual(
          new Set([...invitesBefore, emailA, emailB])
        );
        expect(await driver.wsSettingsInviteRole(emailA)).toBe("Guest");
        expect(await driver.wsSettingsInviteRole(emailB)).toBe("Admin");
        const server = await serverWorkspaceInvitations(seed.workspaceSlug, session);
        expect(server.find((row) => row.email === emailA)?.role).toBe(ROLE.GUEST);
        expect(server.find((row) => row.email === emailB)?.role).toBe(ROLE.ADMIN);
        created.push(
          server.find((row) => row.email === emailA)?.id ?? "",
          server.find((row) => row.email === emailB)?.id ?? ""
        );
      });
    } finally {
      for (const id of created) {
        if (id !== "") await serverCleanupWorkspaceInvitation(seed.workspaceSlug, id, session);
      }
      await sweepInvitationsByPrefix(seed, session, "nf411-inv-");
    }
  }
);

test(
  specTitle(["WSET-009"], "invite control stays hidden for members and guests"),
  { tag: specTags(["WSET-009"]) },
  async ({ driver, seed }) => {
    const member = requireMember(seed);
    const guest = requireGuest(seed);
    await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
    await driver.wsSettingsMembersOpen(seed.workspaceSlug);
    expect(await driver.wsSettingsMembersAddVisible()).toBe(false);

    await switchUser(driver, guest.email, guest.password, seed.workspaceSlug);
    await driver.openPath(`/${seed.workspaceSlug}/settings/members`);
    await expect.poll(() => driver.hasVisibleText("not authorized to view this page")).toBe(true);
    expect(await driver.wsSettingsMembersAddVisible()).toBe(false);
  }
);

test(
  specTitle(["WSET-010"], "pending group collapses, searches, edits roles, copies links, withdraws behind confirm"),
  { tag: specTags(["WSET-010"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const member = requireMember(seed);
    const emailA = uniqueEmail("nf411-pen-a");
    const emailB = uniqueEmail("nf411-pen-b");
    const seeded = await serverCreateWorkspaceInvitations(
      seed.workspaceSlug,
      [
        { email: emailA, role: ROLE.MEMBER },
        { email: emailB, role: ROLE.GUEST },
      ],
      session
    );
    const idA = seeded.find((row) => row.email === emailA)?.id ?? "";
    const idB = seeded.find((row) => row.email === emailB)?.id ?? "";
    // Other runs can leave pending invitations in the shared workspace,
    // so this scenario asserts its own rows against what the group shows
    // before its setup invites land. The UI baseline is taken first: the
    // list endpoint also returns withdrawn rows, so a server-side count
    // would overstate the group.
    let invitesBefore = new Set<string>();
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await adminOpen(driver, seed.workspaceSlug);
      // The setup invites are already listed here, so the baseline is
      // whatever preceded them.
      invitesBefore = new Set(
        (await driver.wsSettingsInvitesVisibleEmails()).filter((email) => email !== emailA && email !== emailB)
      );

      await test.step("group lists pending invitations with roles", async () => {
        await expect.poll(() => driver.wsSettingsInvitesGroupVisible(), { timeout: 30_000 }).toBe(true);
        await expect
          .poll(() => driver.wsSettingsInvitesVisibleEmails(), { timeout: 30_000 })
          .toEqual(expect.arrayContaining([emailA, emailB]));
        expect(await driver.wsSettingsInvitesCount()).toBe(invitesBefore.size + 2);
        expect(new Set(await driver.wsSettingsInvitesVisibleEmails())).toEqual(
          new Set([...invitesBefore, emailA, emailB])
        );
        expect(await driver.wsSettingsInviteRole(emailA)).toBe("Member");
        expect(await driver.wsSettingsInviteRole(emailB)).toBe("Guest");
      });

      await test.step("group collapses and reopens", async () => {
        expect(await driver.wsSettingsInvitesExpanded()).toBe(true);
        await driver.wsSettingsInvitesToggleGroup();
        expect(await driver.wsSettingsInvitesExpanded()).toBe(false);
        expect(await driver.wsSettingsInvitesVisibleEmails()).toEqual([]);
        await driver.wsSettingsInvitesToggleGroup();
        expect(await driver.wsSettingsInvitesExpanded()).toBe(true);
        expect(new Set(await driver.wsSettingsInvitesVisibleEmails())).toEqual(
          new Set([...invitesBefore, emailA, emailB])
        );
      });

      await test.step("search matches pending invitations by email", async () => {
        await driver.wsSettingsMembersSearch(emailA);
        expect(await driver.wsSettingsInvitesVisibleEmails()).toEqual([emailA]);
        expect(await driver.wsSettingsMembersVisibleEmails()).toEqual([]);
        await driver.wsSettingsMembersSearch("");
        expect(new Set(await driver.wsSettingsInvitesVisibleEmails())).toEqual(
          new Set([...invitesBefore, emailA, emailB])
        );
      });

      await test.step("invitation roles change and persist", async () => {
        expect(await driver.wsSettingsInviteRoleOptions(emailA)).toEqual(["Guest", "Member", "Admin"]);
        await driver.wsSettingsInviteSetRole(emailA, "Admin");
        expect(await driver.wsSettingsInviteRole(emailA)).toBe("Admin");
        const server = await serverWorkspaceInvitations(seed.workspaceSlug, session);
        expect(server.find((row) => row.email === emailA)?.role).toBe(ROLE.ADMIN);
      });

      await test.step("invitation menu copies the invite link", async () => {
        expect(await driver.wsSettingsInviteMenuEntries(emailA)).toEqual(["Copy link", "Remove"]);
        const link = await driver.wsSettingsInviteCopyLink(emailA);
        expect(link).toContain(idA);
        expect(link).toContain(seed.workspaceSlug);
      });

      await test.step("withdrawal confirms, then removes server-side", async () => {
        await driver.wsSettingsInviteOpenRemove(emailB);
        expect(await driver.wsSettingsRemoveDialogTitle()).toBe(`Remove ${emailB}?`);
        expect(await driver.wsSettingsRemoveDialogText()).toContain(emailB);
        await driver.wsSettingsRemoveDialogCancel();
        expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
        expect(await driver.wsSettingsInvitesVisibleEmails()).toContain(emailB);
        await driver.wsSettingsInviteOpenRemove(emailB);
        await driver.wsSettingsRemoveDialogConfirm();
        expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
        await expect.poll(() => toastMessage(driver), { timeout: 30_000 }).toContain("Invitation removed");
        expect(await driver.wsSettingsInvitesVisibleEmails()).not.toContain(emailB);
        const server = await serverWorkspaceInvitations(seed.workspaceSlug, session);
        expect(server.some((row) => row.email === emailB)).toBe(false);
      });

      await test.step("failed edits toast and keep the row intact", async () => {
        await serverCleanupWorkspaceInvitation(seed.workspaceSlug, idA, session);
        await driver.wsSettingsInviteSetRole(emailA, "Guest");
        expect(await driver.wsSettingsInviteRole(emailA)).toBe("Admin");
        await expect.poll(() => toastTitle(driver), { timeout: 30_000 }).toBe("Error!");
        const server = await serverWorkspaceInvitations(seed.workspaceSlug, session);
        expect(server.some((row) => row.email === emailA)).toBe(false);
      });

      await test.step("failed withdrawals toast, close, and keep the row", async () => {
        const emailC = uniqueEmail("nf411-pen-c");
        const seededC = await serverCreateWorkspaceInvitations(
          seed.workspaceSlug,
          [{ email: emailC, role: ROLE.MEMBER }],
          session
        );
        const idC = seededC.find((row) => row.email === emailC)?.id ?? "";
        await driver.wsSettingsMembersOpen(seed.workspaceSlug);
        expect(await driver.wsSettingsInvitesVisibleEmails()).toContain(emailC);
        await serverCleanupWorkspaceInvitation(seed.workspaceSlug, idC, session);
        await driver.wsSettingsInviteOpenRemove(emailC);
        await driver.wsSettingsRemoveDialogConfirm();
        expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
        await expect.poll(() => toastTitle(driver), { timeout: 30_000 }).toBe("Error!");
        expect(await driver.wsSettingsInvitesVisibleEmails()).toContain(emailC);
        const server = await serverWorkspaceInvitations(seed.workspaceSlug, session);
        expect(server.some((row) => row.email === emailC)).toBe(false);
        await serverCleanupWorkspaceInvitation(seed.workspaceSlug, idC, session);
      });

      await test.step("members see no pending group in this checkout", async () => {
        await switchUser(driver, member.email, member.password, seed.workspaceSlug);
        await driver.wsSettingsMembersOpen(seed.workspaceSlug);
        expect(await driver.wsSettingsInvitesGroupVisible()).toBe(false);
        expect(await driver.wsSettingsInvitesCount()).toBe(null);
      });
    } finally {
      await serverCleanupWorkspaceInvitation(seed.workspaceSlug, idA, session);
      await serverCleanupWorkspaceInvitation(seed.workspaceSlug, idB, session);
      await sweepInvitationsByPrefix(seed, session, "nf411-pen-");
    }
  }
);

test(
  specTitle(["WSET-011"], "member rows show identity with profile links; no activity entry in this checkout"),
  { tag: specTags(["WSET-011"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const member = requireMember(seed);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.wsSettingsMembersOpen(seed.workspaceSlug);

    await test.step("seed rows render names, roles, auth, dates, avatars, profile links", async () => {
      const mention = await driver.wsSettingsMemberRowFacts("parity-mention@example.com");
      expect(mention.fullName).toBe("Parity Mention");
      expect(mention.displayName).toBe("Parity Mention");
      expect(mention.email).toBe("parity-mention@example.com");
      expect(mention.role).toBe("Member");
      expect(mention.auth).toBe("Email");
      expect(mention.joiningDate).not.toBe(null);
      expect(mention.avatarKind).toBe("initial");
      expect(mention.profileHref).toBe(`/${seed.workspaceSlug}/profile/${member.id}/`);

      const oracle = await driver.wsSettingsMemberRowFacts(seed.email);
      expect(oracle.role).toBe("Admin");
      expect(oracle.avatarKind).toBe("initial");
      expect(oracle.profileHref).toContain(`/${seed.workspaceSlug}/profile/`);

      const guest = await driver.wsSettingsMemberRowFacts("parity-guest@example.com");
      expect(guest.role).toBe("Guest");
      expect(guest.profileHref).toContain(`/${seed.workspaceSlug}/profile/`);
    });

    await test.step("row identity matches the server membership rows", async () => {
      const server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
      for (const email of ["parity-mention@example.com", seed.email, "parity-guest@example.com"]) {
        const ui = await driver.wsSettingsMemberRowFacts(email);
        const row = server.find((entry) => entry.email === email);
        expect(row).not.toBe(undefined);
        expect(ui.displayName).toBe(row?.displayName);
        expect(ui.role).toBe(ROLE_LABEL[row?.role ?? 0]);
        expect(ui.profileHref).toBe(`/${seed.workspaceSlug}/profile/${row?.userId}/`);
      }
    });

    await test.step("no member-activity entry renders in this checkout", async () => {
      expect(await driver.wsSettingsMembersActivityVisible()).toBe(false);
    });
  }
);

test(
  specTitle(["WSET-012"], "admin role changes persist; failed changes toast and keep the row"),
  { tag: specTags(["WSET-012"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const extra = await provisionMember(seed, session, "nf411-w12", ROLE.MEMBER);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await adminOpen(driver, seed.workspaceSlug);

      await test.step("admin changes another member's role and it persists", async () => {
        await expect.poll(() => driver.wsSettingsMemberRoleEditable(extra.email)).toBe(true);
        expect(await driver.wsSettingsMemberRoleOptions(extra.email)).toEqual(["Guest", "Member", "Admin"]);
        await driver.wsSettingsMemberSetRole(extra.email, "Admin");
        const server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
        expect(server.find((row) => row.email === extra.email)?.role).toBe(ROLE.ADMIN);
        await driver.wsSettingsMemberSetRole(extra.email, "Member");
        const back = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
        expect(back.find((row) => row.email === extra.email)?.role).toBe(ROLE.MEMBER);
      });

      await test.step("no self role-change control is offered", async () => {
        expect(await driver.wsSettingsMemberRoleEditable(seed.email)).toBe(false);
      });

      await test.step("failed changes toast and keep the row intact", async () => {
        await serverHardDeleteWorkspaceMember(extra.membershipId);
        await driver.wsSettingsMemberSetRole(extra.email, "Admin");
        const roles = await driver.wsSettingsMembersVisibleRoles();
        expect(roles.find((row) => row.email === extra.email)?.role).toBe("Member");
        await expect.poll(() => toastTitle(driver), { timeout: 30_000 }).toBe("Error!");
        const server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
        expect(server.some((row) => row.email === extra.email)).toBe(false);
      });
    } finally {
      await serverHardDeleteWorkspaceMember(extra.membershipId);
    }
  }
);

test(
  specTitle(["WSET-012"], "member views offer no role controls; server refuses self and member-actor changes"),
  { tag: specTags(["WSET-012"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const member = requireMember(seed);
    const extra = await provisionMember(seed, session, "nf411-w12b", ROLE.MEMBER);
    try {
      await test.step("member views offer no role controls", async () => {
        await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
        await driver.wsSettingsMembersOpen(seed.workspaceSlug);
        expect(await driver.wsSettingsMemberRoleEditable(extra.email)).toBe(false);
        expect(await driver.wsSettingsMemberRoleEditable(member.email)).toBe(false);
      });

      await test.step("server refuses self changes and member-actor changes", async () => {
        const apiBase = parityApiBase();
        const server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
        const adminMs = server.find((row) => row.email === seed.email)?.membershipId ?? "";
        const selfChange = await serverRequestStatus(
          "PATCH",
          `${apiBase}/api/workspaces/${seed.workspaceSlug}/members/${adminMs}/`,
          session,
          { role: ROLE.MEMBER }
        );
        expect(selfChange.status).toBe(400);
        expect(selfChange.bodyText).toContain("own role");
        const memberSession = await signInSession(member.email, member.password);
        const refused = await serverRequestStatus(
          "PATCH",
          `${apiBase}/api/workspaces/${seed.workspaceSlug}/members/${extra.membershipId}/`,
          memberSession,
          { role: ROLE.ADMIN }
        );
        expect(refused.status).toBe(403);
      });
    } finally {
      await serverHardDeleteWorkspaceMember(extra.membershipId);
    }
  }
);

test(
  specTitle(["WSET-013"], "remove confirms then suspends; self-leave lands on a not-authorized gate"),
  { tag: specTags(["WSET-013"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const extra = await provisionMember(seed, session, "nf411-w13", ROLE.MEMBER);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await adminOpen(driver, seed.workspaceSlug);

      await test.step("removing another member confirms, then suspends persistently", async () => {
        expect(await driver.wsSettingsMemberMenuEntry(extra.email)).toBe("Remove");
        await driver.wsSettingsMemberOpenRemove(extra.email);
        expect(await driver.wsSettingsRemoveDialogTitle()).toBe(`Remove ${extra.displayName}?`);
        expect(await driver.wsSettingsRemoveDialogText()).toContain(extra.displayName);
        expect(await driver.wsSettingsRemoveDialogText()).toContain("no longer have access");
        await driver.wsSettingsRemoveDialogCancel();
        expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
        expect(await driver.wsSettingsMembersSuspendedEmails()).not.toContain(extra.email);
        let server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
        expect(server.find((row) => row.email === extra.email)?.isActive).toBe(true);
        await driver.wsSettingsMemberOpenRemove(extra.email);
        await driver.wsSettingsRemoveDialogConfirm();
        expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
        expect(await driver.wsSettingsMembersSuspendedEmails()).toContain(extra.email);
        const facts = await driver.wsSettingsMemberRowFacts(extra.email);
        expect(facts.role).toBe("Suspended");
        expect(facts.avatarKind).toBe("suspended");
        expect(facts.auth).toBe(null);
        expect(facts.joiningDate).toBe(null);
        expect(facts.profileHref).toBe(null);
        expect(await driver.wsSettingsMemberMenuEntry(extra.email)).toBe(null);
        // Removal deactivates server-side; the suspended row survives reload.
        server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
        expect(server.find((row) => row.email === extra.email)?.isActive).toBe(false);
        await driver.wsSettingsMembersOpen(seed.workspaceSlug);
        expect(await driver.wsSettingsMembersSuspendedEmails()).toContain(extra.email);
      });
    } finally {
      await serverHardDeleteWorkspaceMember(extra.membershipId);
    }

    await test.step("self-removal lands on a not-authorized gate for the left workspace", async () => {
      const leaver = await provisionMember(seed, session, "nf411-leave", ROLE.MEMBER);
      try {
        await switchUser(driver, leaver.email, leaver.password, seed.workspaceSlug);
        await driver.wsSettingsMembersOpen(seed.workspaceSlug);
        expect(await driver.wsSettingsMemberMenuEntry(leaver.email)).toBe("Leave");
        await driver.wsSettingsMemberOpenRemove(leaver.email);
        expect(await driver.wsSettingsRemoveDialogTitle()).toBe("Leave workspace?");
        await driver.wsSettingsRemoveDialogConfirm();
        expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
        // The redirect targets the just-left workspace home (from the
        // stale workspace list), which renders a not-authorized gate
        // offering pending invites or workspace creation — not the
        // workspace picker itself.
        await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toBe(`/${seed.workspaceSlug}/`);
        await expect.poll(() => driver.hasVisibleText("Not Authorized!")).toBe(true);
        expect(await driver.hasVisibleText("not a member of this workspace")).toBe(true);
        const server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
        expect(server.find((row) => row.email === leaver.email)?.isActive).toBe(false);
      } finally {
        await serverHardDeleteWorkspaceMember(leaver.membershipId);
      }
    });
  }
);

test(
  specTitle(["WSET-013"], "failed member removals toast, close, and keep the row"),
  { tag: specTags(["WSET-013"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const fragile = await provisionMember(seed, session, "nf411-w13f", ROLE.GUEST);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await adminOpen(driver, seed.workspaceSlug);

      await serverHardDeleteWorkspaceMember(fragile.membershipId);
      await driver.wsSettingsMemberOpenRemove(fragile.email);
      await driver.wsSettingsRemoveDialogConfirm();
      expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
      await expect.poll(() => toastTitle(driver), { timeout: 30_000 }).toBe("Error!");
      await expect.poll(() => toastMessage(driver), { timeout: 30_000 }).toContain("required object does not exist");
      expect(await driver.wsSettingsMembersVisibleEmails()).toContain(fragile.email);
      expect(await driver.wsSettingsMembersSuspendedEmails()).not.toContain(fragile.email);
      const server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
      expect(server.some((row) => row.email === fragile.email)).toBe(false);
    } finally {
      await serverHardDeleteWorkspaceMember(fragile.membershipId);
    }
  }
);

test(
  specTitle(["WSET-014"], "bug: NEWFRONT-422 no skeleton renders while the roster loads"),
  { tag: specTags(["WSET-014"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

    // Intended: a skeleton loader shows until members and invitations
    // resolve. Observed: the roster area stays empty until the rows land
    // (the loader guard can never pass). The new frontend implements the
    // intended skeleton, not the observed gap (gate-aligned target
    // behavior, NEWFRONT-537).
    await driver.wsSettingsMembersDelayNextList(8_000);
    const opened = driver.wsSettingsMembersOpen(seed.workspaceSlug);
    await driver.page.waitForTimeout(4_000);
    if (driver.target === "web_new") {
      expect(await driver.wsSettingsMembersLoaderVisible()).toBe(true);
      expect(await driver.wsSettingsMembersVisibleEmails()).toEqual([]);
      expect(await driver.wsSettingsMembersNoMatchVisible()).toBe(false);
      await opened;
      expect(await driver.wsSettingsMembersLoaderVisible()).toBe(false);
      expect((await driver.wsSettingsMembersVisibleEmails()).length).toBeGreaterThan(0);
    } else {
      expect(await driver.wsSettingsMembersLoaderVisible()).toBe(false);
      expect(await driver.wsSettingsMembersVisibleEmails()).toEqual([]);
      expect(await driver.wsSettingsMembersNoMatchVisible()).toBe(false);
      await opened;
      expect(await driver.wsSettingsMembersLoaderVisible()).toBe(false);
      expect((await driver.wsSettingsMembersVisibleEmails()).length).toBeGreaterThan(0);
    }
    const server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
    expect(server.length).toBeGreaterThan(0);
  }
);

test(
  specTitle(["WSET-014"], "no-match message appears without touching data"),
  { tag: specTags(["WSET-014"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await adminOpen(driver, seed.workspaceSlug);
    const before = await serverWorkspaceMembersFull(seed.workspaceSlug, session);

    await driver.wsSettingsMembersSearch("zzz-no-such-member-zzz");
    expect(await driver.wsSettingsMembersNoMatchVisible()).toBe(true);
    expect(await driver.wsSettingsMembersNoMatchText()).toBe("No matching members");
    expect(await driver.wsSettingsMembersVisibleEmails()).toEqual([]);
    const after = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
    expect(after).toEqual(before);
    await driver.wsSettingsMembersSearch("");
    expect(await driver.wsSettingsMembersNoMatchVisible()).toBe(false);
    expect((await driver.wsSettingsMembersVisibleEmails()).length).toBe(before.filter((row) => !row.isBot).length);
  }
);

test(
  specTitle(["WSET-015"], "no billing upsell entry renders in this checkout for any role"),
  { tag: specTags(["WSET-015"]) },
  async ({ driver, seed }) => {
    // Cloud-tagged row: the OSS build and the cloud build both render no
    // upsell affordance in the members header (the cloud overlay's billing
    // action component is empty), so the same absence is asserted on both.
    const session = await seededSession(seed, seed.email);
    const member = await provisionEditionMember(seed, session, "wset015-member", ROLE.MEMBER);
    const server = await serverWorkspaceMembersFull(seed.workspaceSlug, session);
    expect(server.some((row) => row.email === seed.email && row.role === ROLE.ADMIN)).toBe(true);

    await test.step("admins see no billing entry in the members header", async () => {
      await openWorkspacePage(driver, seed, `/${seed.workspaceSlug}/settings/members`, {
        email: seed.email,
        session,
        password: seed.password,
      });
      await adminOpen(driver, seed.workspaceSlug);
      expect(await driver.wsSettingsMembersBillingVisible()).toBe(false);
    });

    await test.step("members see no billing entry either", async () => {
      await openWorkspacePage(driver, seed, `/${seed.workspaceSlug}/settings/members`, member);
      await driver.wsSettingsMembersOpen(seed.workspaceSlug);
      expect(await driver.wsSettingsMembersAddVisible()).toBe(false);
      expect(await driver.wsSettingsMembersBillingVisible()).toBe(false);
    });

    // The member row belongs to the shared scratch stack; the roster
    // assertions here never depend on the roster being empty.
  }
);
