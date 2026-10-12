// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-391): project settings members tab — the
// roster table with profile-linked avatars, client-side search,
// session-persisted role filters and per-column sort, the admin multi-row
// invite dialog with
// per-row role caps, inline role edits under the stated exceptions, remove
// versus leave confirmations, save-on-change project defaults, and the
// loading/empty/failure states. Rows: PSET-018–027.
//
// Observed behavior notes: the table shows full name (avatar links to the
// member profile), display name, email, role word and joining date;
// project guests and workspace outsiders get the not-authorized view. Sort
// and filter choices persist per project across tab visits within the
// session; a full reload resets them (NEWFRONT-409). The invite dialog
// filters role options by the co-worker's workspace role
// (workspace guests enter at guest only, workspace admins at admin only,
// workspace members get the full map); there is no inviter-level cap in
// the dialog or on the create endpoint (NEWFRONT-399). A workspace admin
// sees the editor on every row except another workspace admin's but only
// their own write lands (NEWFRONT-401); a project admin may change
// anyone's except their own and except a workspace admin's; the server
// additionally refuses promotions to a role at or above the inviter's own
// level, and there is no last-admin guard. Removal is a soft deactivation
// behind a permanent-access-loss warning; leaving uses the leave endpoint
// and lands on the projects list. A failed role change toasts the server
// message durably; a failed removal instead kills the page (NEWFRONT-406),
// so no scenario submits one. Membership mutations below restore the
// seeded membership through the API (in finally blocks) so later scenarios
// see a clean stack.
import { test, expect } from "../fixtures";
import {
  PROJECT_ROLE_ADMIN,
  PROJECT_ROLE_GUEST,
  PROJECT_ROLE_MEMBER,
  serverAddProjectMembers,
  serverPatchProjectMember,
  serverPatchWorkspaceMember,
  serverProjectMemberByEmail,
  serverProjectMemberProfiles,
  serverProjectSettings,
  serverProvisionWorkspaceMember,
  serverCleanupWorkspaceMember,
  serverRemoveProjectMember,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROLE_WORD: Record<number, string> = {
  [PROJECT_ROLE_ADMIN]: "Admin",
  [PROJECT_ROLE_MEMBER]: "Member",
  [PROJECT_ROLE_GUEST]: "Guest",
};

test(
  specTitle(["PSET-018"], "member table shows every member with profile-linked avatars; guests see the denial view"),
  { tag: specTags(["PSET-018"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const serverMembers = await serverProjectMemberProfiles(seed.workspaceSlug, seed.projectId, session);

    await test.step("the table shows every member with the five columns", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      expect(await driver.settingsMembersDenied()).toBe(false);
      // The heading settles before the rows load; wait for the roster.
      await expect
        .poll(async () => (await driver.settingsMembersRows()).length, { timeout: 30_000 })
        .toBe(serverMembers.length);
      const rows = await driver.settingsMembersRows();
      expect(rows.map((r) => r.email)).toEqual(serverMembers.map((m) => m.email));
      for (const row of rows) {
        const server = serverMembers.find((m) => m.email === row.email);
        expect(server).toBeDefined();
        expect(row.displayName).toBe(server?.displayName);
        expect(row.fullName).toBe(`${server?.firstName} ${server?.lastName}`.trim());
        expect(row.roleLabel).toBe(ROLE_WORD[server?.role ?? 0]);
        expect(row.joinedText.trim().length).toBeGreaterThan(0);
        expect(row.profileHref).toContain(`/profile/${server?.memberId}`);
      }
    });

    await test.step("a project guest gets the not-authorized view", async () => {
      await driver.dropSession();
      await driver.rulesEnsureSignedIn(seed.guestEmail ?? "", seed.guestPassword ?? "", seed.workspaceSlug);
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      expect(await driver.settingsMembersDenied()).toBe(true);
    });
  }
);

test(
  specTitle(["PSET-019"], "member search filters client-side with no per-keystroke requests"),
  { tag: specTags(["PSET-019"]) },
  async ({ driver, seed }) => {
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
    await expect.poll(() => driver.settingsMembersRows(), { timeout: 30_000 }).not.toHaveLength(0);
    const all = await driver.settingsMembersRows();
    expect(all.length).toBeGreaterThan(1);
    const mention = seed.mentionMember?.displayName ?? "Mention";

    await test.step("typing narrows instantly and case-insensitively", async () => {
      await driver.settingsMembersSearchFill("mEnTiOn");
      await expect.poll(() => driver.settingsMembersRows(), { timeout: 15_000 }).toHaveLength(1);
      const rows = await driver.settingsMembersRows();
      expect(rows[0]?.displayName).toBe(mention);
    });

    await test.step("typing issues no member-list requests", async () => {
      let gets = 0;
      await driver.page.route("**/api/workspaces/*/projects/*/members*", async (route) => {
        if (route.request().method() === "GET") gets += 1;
        await route.continue();
      });
      await driver.settingsMembersSearchFill("");
      await driver.settingsMembersSearchFill("par");
      await driver.page.waitForTimeout(1500);
      await driver.settingsMembersSearchFill("pari");
      await driver.page.waitForTimeout(1500);
      expect(gets).toBe(0);
      await driver.page.unrouteAll({ behavior: "wait" }).catch(() => {});
    });

    await test.step("clearing restores the full list", async () => {
      await driver.settingsMembersSearchFill("");
      await expect.poll(async () => (await driver.settingsMembersRows()).length, { timeout: 15_000 }).toBe(all.length);
    });
  }
);

test(
  specTitle(["PSET-020"], "role filter narrows by role, shows an indicator, and persists per project"),
  { tag: specTags(["PSET-020"]) },
  async ({ driver, seed }) => {
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);

    await test.step("the dropdown offers one checkbox per role", async () => {
      await driver.settingsMembersFiltersOpen();
      expect(await driver.settingsMembersFilterOptions()).toEqual(["Admin", "Member", "Guest"]);
      expect(await driver.settingsMembersFilterActive()).toBe(false);
    });

    await test.step("checking Member narrows the table and marks the button", async () => {
      await driver.settingsMembersFilterToggle("Member");
      expect(await driver.settingsMembersFilterChecked("Member")).toBe(true);
      await driver.settingsMembersFiltersClose();
      await expect.poll(() => driver.settingsMembersRows(), { timeout: 15_000 }).toHaveLength(1);
      expect((await driver.settingsMembersRows())[0]?.roleLabel).toBe("Member");
      expect(await driver.settingsMembersFilterActive()).toBe(true);
    });

    await test.step("the filter survives moving between tabs", async () => {
      // Role filters live in the per-project session store: a full reload
      // resets them, while in-app tab visits keep them applied.
      await driver.settingsMembersGotoTab("General");
      await driver.settingsMembersGotoTab("Members");
      await expect.poll(() => driver.settingsMembersRows(), { timeout: 15_000 }).toHaveLength(1);
      expect(await driver.settingsMembersFilterActive()).toBe(true);
    });

    await test.step("unchecking restores every row and clears the indicator", async () => {
      await driver.settingsMembersFiltersOpen();
      await driver.settingsMembersFilterToggle("Member");
      await driver.settingsMembersFiltersClose();
      await expect.poll(() => driver.settingsMembersRows(), { timeout: 15_000 }).toHaveLength(3);
      expect(await driver.settingsMembersFilterActive()).toBe(false);
    });
  }
);

test(
  specTitle(["PSET-021"], "column sort orders, clears, and persists with direction feedback"),
  { tag: specTags(["PSET-021"]) },
  async ({ driver, seed }) => {
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
    // Default order before any sort: the reload step asserts the table
    // returns to exactly this shape.
    await expect.poll(() => driver.settingsMembersRows(), { timeout: 15_000 }).toHaveLength(3);
    const defaultOrder = (await driver.settingsMembersRows()).map((r) => r.displayName);
    expect(await driver.settingsMembersSortMarker("Display name")).toBe("none");

    await test.step("ascending orders display names A to Z", async () => {
      await driver.settingsMembersSortOpen("Display name");
      const items = await driver.settingsMembersSortItems();
      const ascending = items.find((t) => t.startsWith("A")) ?? "";
      expect(ascending).not.toBe("");
      await driver.settingsMembersSortPick(ascending);
      await expect
        .poll(async () => (await driver.settingsMembersRows()).map((r) => r.displayName), { timeout: 15_000 })
        .toEqual(["Parity Guest", "Parity Mention", "Parity Oracle"]);
      await expect.poll(() => driver.settingsMembersSortMarker("Display name"), { timeout: 15_000 }).toBe("ascending");
      expect(await driver.settingsMembersSortMarker("Email")).toBe("none");
    });

    await test.step("descending reverses the order", async () => {
      await driver.settingsMembersSortOpen("Display name");
      const items = await driver.settingsMembersSortItems();
      const descending = items.find((t) => t.startsWith("Z")) ?? "";
      expect(descending).not.toBe("");
      await driver.settingsMembersSortPick(descending);
      await expect
        .poll(async () => (await driver.settingsMembersRows()).map((r) => r.displayName), { timeout: 15_000 })
        .toEqual(["Parity Oracle", "Parity Mention", "Parity Guest"]);
      await expect.poll(() => driver.settingsMembersSortMarker("Display name"), { timeout: 15_000 }).toBe("descending");
    });

    await test.step("the choice survives moving between tabs, then clears", async () => {
      // Like the role filters, the sort choice lives in the per-project
      // session store: tab visits keep it, a full reload resets it.
      await driver.settingsMembersGotoTab("General");
      await driver.settingsMembersGotoTab("Members");
      await expect
        .poll(async () => (await driver.settingsMembersRows()).map((r) => r.displayName), { timeout: 15_000 })
        .toEqual(["Parity Oracle", "Parity Mention", "Parity Guest"]);
      await expect.poll(() => driver.settingsMembersSortMarker("Display name"), { timeout: 15_000 }).toBe("descending");
      await driver.settingsMembersSortOpen("Display name");
      const items = await driver.settingsMembersSortItems();
      const clear = items.find((t) => t.includes("Clear")) ?? "";
      expect(clear).not.toBe("");
      await driver.settingsMembersSortPick(clear);
      await expect.poll(() => driver.settingsMembersRows(), { timeout: 15_000 }).toHaveLength(3);
      await expect.poll(() => driver.settingsMembersSortMarker("Display name"), { timeout: 15_000 }).toBe("none");
    });

    await test.step("a full reload resets to the default order (NEWFRONT-409)", async () => {
      // The row acceptance claims reload survival, but the store is
      // session-only: re-apply a sort, reload, and pin the reset.
      await driver.settingsMembersSortOpen("Display name");
      const items = await driver.settingsMembersSortItems();
      const descending = items.find((t) => t.startsWith("Z")) ?? "";
      expect(descending).not.toBe("");
      await driver.settingsMembersSortPick(descending);
      await expect.poll(() => driver.settingsMembersSortMarker("Display name"), { timeout: 15_000 }).toBe("descending");
      await driver.reloadPage();
      await expect.poll(() => driver.settingsMembersRows(), { timeout: 30_000 }).toHaveLength(3);
      await expect
        .poll(async () => (await driver.settingsMembersRows()).map((r) => r.displayName), { timeout: 15_000 })
        .toEqual(defaultOrder);
      expect(await driver.settingsMembersSortMarker("Display name")).toBe("none");
    });
  }
);

test(
  specTitle(["PSET-022"], "admin invite dialog adds workspace non-members in bulk with per-row role caps"),
  { tag: specTags(["PSET-022"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const guestEmail = seed.guestEmail ?? "";
    // The seeded guest must leave the project so the picker can offer them;
    // they are re-added afterwards to restore the seeded membership.
    const guestBefore = await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, guestEmail, session);
    const fresh = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf391add", session);
    const freshDisplay = (await serverWorkspaceMembers(seed.workspaceSlug, session)).find(
      (m) => m.userId === fresh.userId
    )?.displayName;
    expect(freshDisplay).toBeDefined();
    try {
      if (guestBefore !== null) {
        await serverRemoveProjectMember(seed.workspaceSlug, seed.projectId, guestBefore.membershipId, session);
      }
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      expect(await driver.settingsMembersAddVisible()).toBe(true);

      await test.step("empty rows refuse to submit per row", async () => {
        await driver.settingsMembersOpenInvite();
        expect(await driver.settingsMembersInviteRowCount()).toBe(1);
        await driver.settingsMembersInviteAttemptSubmit();
        await expect.poll(() => driver.settingsMembersInviteRowError(0), { timeout: 10_000 }).not.toBe("");
        expect(await driver.settingsMembersInviteOpen()).toBe(true);
      });

      await test.step("current members are excluded while outsiders are offered", async () => {
        await driver.settingsMembersInviteOpenMemberPicker(0);
        const options = await driver.settingsMembersInviteMemberOptions();
        expect(options.some((t) => t.includes(freshDisplay ?? ""))).toBe(true);
        expect(options.some((t) => t.includes("Parity Oracle"))).toBe(false);
        await driver.settingsMembersInviteCloseMemberPicker(0);
      });

      await test.step("picking a co-worker defaults the role to their workspace role", async () => {
        await driver.settingsMembersInvitePickMember(0, freshDisplay ?? "");
        await expect
          .poll(async () => driver.settingsMembersInviteRoleOptions(0), { timeout: 15_000 })
          .toContain("Member");
      });

      await test.step("a workspace guest is capped at guest level", async () => {
        await driver.settingsMembersInviteAddRow();
        expect(await driver.settingsMembersInviteRowCount()).toBe(2);
        const guestDisplay = (await serverWorkspaceMembers(seed.workspaceSlug, session)).find((m) =>
          m.displayName.includes("Guest")
        )?.displayName;
        await driver.settingsMembersInvitePickMember(1, guestDisplay ?? "Guest");
        await expect
          .poll(async () => driver.settingsMembersInviteRoleOptions(1), { timeout: 15_000 })
          .toEqual(["Guest"]);
        // Drop the guest row again: this scenario adds only the fresh member.
        await driver.settingsMembersInviteRemoveRow(1);
        expect(await driver.settingsMembersInviteRowCount()).toBe(1);
      });

      await test.step("submit bulk-adds the row with the chosen role", async () => {
        await driver.settingsMembersInvitePickRole(0, "Member");
        await driver.settingsMembersInviteSubmit();
        await expect
          .poll(async () => (await driver.settingsMembersRows()).map((r) => r.email), { timeout: 15_000 })
          .toContain(fresh.email);
        const added = await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, fresh.email, session);
        expect(added?.role).toBe(PROJECT_ROLE_MEMBER);
      });
    } finally {
      const added = await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, fresh.email, session);
      if (added !== null) {
        await serverRemoveProjectMember(seed.workspaceSlug, seed.projectId, added.membershipId, session);
      }
      const wsMembers = await serverWorkspaceMembers(seed.workspaceSlug, session);
      const wsRow = wsMembers.find((m) => m.userId === fresh.userId);
      if (wsRow !== undefined) await serverCleanupWorkspaceMember(seed.workspaceSlug, wsRow.membershipId, session);
      if (guestBefore !== null) {
        await serverAddProjectMembers(
          seed.workspaceSlug,
          seed.projectId,
          [{ memberId: guestBefore.memberId, role: PROJECT_ROLE_GUEST }],
          session
        );
      }
    }
  }
);

test(
  specTitle(["PSET-023"], "inline role edits follow the admin exceptions with no last-admin guard"),
  { tag: specTags(["PSET-023"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const mentionName = seed.mentionMember?.displayName ?? "Parity Mention";
    const mentionEmail = seed.mentionMember?.email ?? "";
    const mentionPassword = seed.mentionMember?.password ?? "";
    const mentionId = seed.mentionMember?.id ?? "";
    const ownerName = "Parity Oracle";
    const guestName = "Parity Guest";
    const mentionProjectRole = async () =>
      (await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, mentionEmail, session))?.role;
    const mentionWsMembershipId = async () =>
      (await serverWorkspaceMembers(seed.workspaceSlug, session)).find((m) => m.userId === mentionId)?.membershipId ??
      "";
    // Converge the seeded member's roles from any staging state (project
    // first: while workspace-admin the project write always lands).
    const convergeSeedRoles = async () => {
      const projMid =
        (await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, mentionEmail, session))?.membershipId ??
        "";
      await serverPatchProjectMember(seed.workspaceSlug, seed.projectId, projMid, session, PROJECT_ROLE_MEMBER);
      await serverPatchWorkspaceMember(seed.workspaceSlug, await mentionWsMembershipId(), session, PROJECT_ROLE_MEMBER);
    };

    await test.step("a project admin edits others, with per-target option caps", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      // The seeded owner is both workspace admin and project admin, so
      // their own cell is editable through the workspace-admin clause.
      expect(await driver.settingsMembersRoleEditable(ownerName)).toBe(true);
      expect(await driver.settingsMembersRoleEditable(mentionName)).toBe(true);
      expect(await driver.settingsMembersRoleEditable(guestName)).toBe(true);
      await driver.settingsMembersRoleEditorOpen(mentionName);
      expect(await driver.settingsMembersRoleEditorOptions()).toEqual(["Guest", "Member", "Admin"]);
      await driver.settingsMembersRoleEditorPick("Member");
      await driver.settingsMembersRoleEditorOpen(guestName);
      // A workspace guest is capped to the guest option even for admins.
      expect(await driver.settingsMembersRoleEditorOptions()).toEqual(["Guest"]);
      await driver.settingsMembersRoleEditorPick("Guest");
      await expect.poll(() => driver.settingsMembersRoleText(guestName), { timeout: 15_000 }).toBe("Guest");
    });

    await test.step("a forbidden promotion toasts the server message", async () => {
      await driver.settingsMembersRoleEditorOpen(mentionName);
      await driver.settingsMembersRoleEditorPick("Admin");
      await expect
        .poll(
          async () => {
            const toast = await driver.rulesLastToast();
            return toast ? `${toast.title} ${toast.message}` : "";
          },
          { timeout: 15_000 }
        )
        .toContain("You cannot assign a role equal to or higher than your own");
      expect(await mentionProjectRole()).toBe(PROJECT_ROLE_MEMBER);
      await expect.poll(() => driver.settingsMembersRoleText(mentionName), { timeout: 15_000 }).toBe("Member");
    });

    await test.step("an allowed demotion persists and restores", async () => {
      await driver.settingsMembersRoleEditorOpen(mentionName);
      await driver.settingsMembersRoleEditorPick("Guest");
      await expect.poll(() => driver.settingsMembersRoleText(mentionName), { timeout: 15_000 }).toBe("Guest");
      expect(await mentionProjectRole()).toBe(PROJECT_ROLE_GUEST);
      await driver.settingsMembersRoleEditorOpen(mentionName);
      await driver.settingsMembersRoleEditorPick("Member");
      await expect.poll(() => driver.settingsMembersRoleText(mentionName), { timeout: 15_000 }).toBe("Member");
      expect(await mentionProjectRole()).toBe(PROJECT_ROLE_MEMBER);
    });

    await test.step("the sole admin demotes themself: no last-admin guard", async () => {
      const ownerMid =
        (await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, seed.email, session))?.membershipId ?? "";
      try {
        await driver.settingsMembersRoleEditorOpen(ownerName);
        await driver.settingsMembersRoleEditorPick("Member");
        await expect.poll(() => driver.settingsMembersRoleText(ownerName), { timeout: 15_000 }).toBe("Member");
        expect((await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, seed.email, session))?.role).toBe(
          PROJECT_ROLE_MEMBER
        );
        // A demoted workspace admin still edits their own cell: restore it.
        await driver.settingsMembersRoleEditorOpen(ownerName);
        await driver.settingsMembersRoleEditorPick("Admin");
        await expect.poll(() => driver.settingsMembersRoleText(ownerName), { timeout: 15_000 }).toBe("Admin");
        expect((await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, seed.email, session))?.role).toBe(
          PROJECT_ROLE_ADMIN
        );
      } finally {
        await serverPatchProjectMember(seed.workspaceSlug, seed.projectId, ownerMid, session, PROJECT_ROLE_ADMIN);
      }
    });

    await test.step("a project admin sees their own and a workspace admin's cell read-only", async () => {
      // Stage a project admin who is not a workspace admin: the workspace
      // promotion lets the project promotion land, then is taken back
      // (member<->admin workspace moves leave project roles untouched).
      const wsMid = await mentionWsMembershipId();
      try {
        await serverPatchWorkspaceMember(seed.workspaceSlug, wsMid, session, PROJECT_ROLE_ADMIN);
        const projMid =
          (await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, mentionEmail, session))?.membershipId ??
          "";
        await serverPatchProjectMember(seed.workspaceSlug, seed.projectId, projMid, session, PROJECT_ROLE_ADMIN);
        await serverPatchWorkspaceMember(seed.workspaceSlug, wsMid, session, PROJECT_ROLE_MEMBER);
        expect(await mentionProjectRole()).toBe(PROJECT_ROLE_ADMIN);
        await driver.dropSession();
        await driver.rulesEnsureSignedIn(mentionEmail, mentionPassword, seed.workspaceSlug);
        await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
        expect(await driver.settingsMembersRoleEditable(mentionName)).toBe(false);
        expect(await driver.settingsMembersRoleEditable(ownerName)).toBe(false);
        expect(await driver.settingsMembersRoleEditable(guestName)).toBe(true);
      } finally {
        // Sandwich: only a workspace admin target (or holder) lets the
        // project demotion land, so promote, demote, then take it back.
        await serverPatchWorkspaceMember(seed.workspaceSlug, wsMid, session, PROJECT_ROLE_ADMIN);
        const projMid =
          (await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, mentionEmail, session))?.membershipId ??
          "";
        await serverPatchProjectMember(seed.workspaceSlug, seed.projectId, projMid, session, PROJECT_ROLE_MEMBER);
        await serverPatchWorkspaceMember(seed.workspaceSlug, wsMid, session, PROJECT_ROLE_MEMBER);
      }
    });

    await test.step("a workspace admin edits only their own role end to end", async () => {
      const wsMid = await mentionWsMembershipId();
      try {
        await serverPatchWorkspaceMember(seed.workspaceSlug, wsMid, session, PROJECT_ROLE_ADMIN);
        await driver.dropSession();
        await driver.rulesEnsureSignedIn(mentionEmail, mentionPassword, seed.workspaceSlug);
        await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
        // Display layer: the workspace admin's effective project role is
        // admin, so the editor shows on every row except a workspace
        // admin's (NEWFRONT-401); the writes below prove only the
        // own-role one lands.
        expect(await driver.settingsMembersRoleEditable(mentionName)).toBe(true);
        expect(await driver.settingsMembersRoleEditable(ownerName)).toBe(false);
        expect(await driver.settingsMembersRoleEditable(guestName)).toBe(true);
        // Another member's write fails: the server governs by the real
        // project role. (The guest row offers only the guest option, so
        // even the no-op write is refused.)
        await driver.settingsMembersRoleEditorOpen(guestName);
        expect(await driver.settingsMembersRoleEditorOptions()).toEqual(["Guest"]);
        await driver.settingsMembersRoleEditorPick("Guest");
        await expect
          .poll(
            async () => {
              const toast = await driver.rulesLastToast();
              return toast ? `${toast.title} ${toast.message}` : "";
            },
            { timeout: 15_000 }
          )
          .toContain("You do not have permission to update roles");
        // The own-role edit persists (self-target writes bypass the server
        // checks) and restores through the UI as well.
        await driver.settingsMembersRoleEditorOpen(mentionName);
        await driver.settingsMembersRoleEditorPick("Admin");
        await expect.poll(() => driver.settingsMembersRoleText(mentionName), { timeout: 15_000 }).toBe("Admin");
        expect(await mentionProjectRole()).toBe(PROJECT_ROLE_ADMIN);
        await driver.settingsMembersRoleEditorOpen(mentionName);
        await driver.settingsMembersRoleEditorPick("Member");
        await expect.poll(() => driver.settingsMembersRoleText(mentionName), { timeout: 15_000 }).toBe("Member");
        expect(await mentionProjectRole()).toBe(PROJECT_ROLE_MEMBER);
      } finally {
        await convergeSeedRoles();
      }
    });

    await test.step("a plain member sees read-only role text", async () => {
      await convergeSeedRoles();
      await driver.dropSession();
      await driver.rulesEnsureSignedIn(mentionEmail, mentionPassword, seed.workspaceSlug);
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      expect(await driver.settingsMembersRoleEditable(mentionName)).toBe(false);
      expect(await driver.settingsMembersRoleEditable(ownerName)).toBe(false);
      expect(await driver.settingsMembersRoleEditable(guestName)).toBe(false);
      expect(await driver.settingsMembersRoleText(mentionName)).toBe("Member");
    });
  }
);

test(
  specTitle(["PSET-024"], "remove via hover menu confirms with a permanent-access warning"),
  { tag: specTags(["PSET-024"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const mentionName = seed.mentionMember?.displayName ?? "Parity Mention";
    const mentionEmail = seed.mentionMember?.email ?? "";

    const restoreMention = async () => {
      const mentionId = seed.mentionMember?.id ?? "";
      await serverAddProjectMembers(
        seed.workspaceSlug,
        seed.projectId,
        [{ memberId: mentionId, role: PROJECT_ROLE_MEMBER }],
        session
      );
    };

    await test.step("cancel keeps the membership", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      await driver.settingsMembersOpenRowMenu(mentionName);
      expect(await driver.settingsMembersRowMenuItems()).toEqual(["Remove"]);
      await driver.settingsMembersRowMenuPick("Remove");
      const confirm = await driver.settingsMembersConfirmText();
      expect(confirm).toContain(mentionName);
      expect(confirm).toContain("no longer have access");
      await driver.settingsMembersConfirmCancel();
      expect(
        (await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, mentionEmail, session)) !== null
      ).toBe(true);
    });

    await test.step("confirm removes the row and deactivates the membership", async () => {
      try {
        await driver.settingsMembersOpenRowMenu(mentionName);
        await driver.settingsMembersRowMenuPick("Remove");
        await driver.settingsMembersConfirmSubmit();
        await expect
          .poll(async () => (await driver.settingsMembersRows()).map((r) => r.email), { timeout: 15_000 })
          .not.toContain(mentionEmail);
        expect(await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, mentionEmail, session)).toBe(null);
      } finally {
        // Restore the seeded membership (re-adding an active member is a
        // harmless role no-op, so this is safe on any path).
        await restoreMention();
      }
      expect((await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, mentionEmail, session))?.role).toBe(
        PROJECT_ROLE_MEMBER
      );
    });
  }
);

test(
  specTitle(["PSET-025"], "leave from one's own row uses the leave endpoint and lands on the list"),
  { tag: specTags(["PSET-025"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const mentionName = seed.mentionMember?.displayName ?? "Parity Mention";
    const mentionEmail = seed.mentionMember?.email ?? "";

    const restoreMention = async () => {
      const mentionId = seed.mentionMember?.id ?? "";
      await serverAddProjectMembers(
        seed.workspaceSlug,
        seed.projectId,
        [{ memberId: mentionId, role: PROJECT_ROLE_MEMBER }],
        session
      );
    };
    try {
      await driver.rulesEnsureSignedIn(
        seed.mentionMember?.email ?? "",
        seed.mentionMember?.password ?? "",
        seed.workspaceSlug
      );
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      await driver.settingsMembersOpenRowMenu(mentionName);
      expect(await driver.settingsMembersRowMenuItems()).toEqual(["Leave"]);
      await driver.settingsMembersRowMenuPick("Leave");
      const confirm = await driver.settingsMembersConfirmText();
      expect(confirm).toContain("leave");
      await driver.settingsMembersConfirmSubmit();
      await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toContain("/projects");
      expect(await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, mentionEmail, session)).toBe(null);
    } finally {
      // Restore the seeded membership via the API.
      await restoreMention();
    }
    // The restored member lands back on the page without the leave round-trip.
    await driver.dropSession();
    await driver.rulesEnsureSignedIn(
      seed.mentionMember?.email ?? "",
      seed.mentionMember?.password ?? "",
      seed.workspaceSlug
    );
    await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
    expect((await driver.settingsMembersRows()).map((r) => r.email)).toContain(mentionEmail);
  }
);

test(
  specTitle(["PSET-026"], "project defaults save on change; non-admins see disabled controls"),
  { tag: specTags(["PSET-026"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const mentionName = seed.mentionMember?.displayName ?? "Parity Mention";
    const mentionId = seed.mentionMember?.id ?? "";

    await test.step("lead and assignee pickers persist on change and clear to none", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      expect(await driver.settingsMembersDefaultsVisible()).toBe(true);
      expect(await driver.settingsMembersLeadDisabled()).toBe(false);

      await driver.settingsMembersLeadPick(mentionName);
      expect((await serverProjectSettings(seed.workspaceSlug, seed.projectId, session)).projectLeadId).toBe(mentionId);
      await driver.settingsMembersAssigneePick(mentionName);
      expect((await serverProjectSettings(seed.workspaceSlug, seed.projectId, session)).defaultAssigneeId).toBe(
        mentionId
      );

      await driver.settingsMembersLeadPick("None");
      expect((await serverProjectSettings(seed.workspaceSlug, seed.projectId, session)).projectLeadId).toBe(null);
      await driver.settingsMembersAssigneePick("None");
      expect((await serverProjectSettings(seed.workspaceSlug, seed.projectId, session)).defaultAssigneeId).toBe(null);
    });

    await test.step("the guest-visibility toggle flips viewing rights", async () => {
      const before = (await serverProjectSettings(seed.workspaceSlug, seed.projectId, session)).guestViewAll;
      await driver.settingsMembersGuestToggle();
      expect((await serverProjectSettings(seed.workspaceSlug, seed.projectId, session)).guestViewAll).toBe(!before);
      // Restore the seeded flag.
      await driver.settingsMembersGuestToggle();
      expect((await serverProjectSettings(seed.workspaceSlug, seed.projectId, session)).guestViewAll).toBe(before);
    });

    await test.step("non-admins see the controls disabled", async () => {
      await driver.dropSession();
      await driver.rulesEnsureSignedIn(
        seed.mentionMember?.email ?? "",
        seed.mentionMember?.password ?? "",
        seed.workspaceSlug
      );
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      expect(await driver.settingsMembersLeadDisabled()).toBe(true);
      expect(await driver.settingsMembersGuestToggleDisabled()).toBe(true);
    });
  }
);

test(
  specTitle(["PSET-027"], "loading skeleton, no-match notice, error toasts, and dialog resets"),
  { tag: specTags(["PSET-027"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const mentionName = seed.mentionMember?.displayName ?? "Parity Mention";
    const mentionEmail = seed.mentionMember?.email ?? "";

    await test.step("a skeleton stands in while the roster loads", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsMembersDelayLoad(4000);
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      await expect.poll(() => driver.settingsMembersSkeletonVisible(), { timeout: 15_000 }).toBe(true);
      await expect.poll(() => driver.settingsMembersRows(), { timeout: 30_000 }).toHaveLength(3);
      await driver.page.unrouteAll({ behavior: "wait" }).catch(() => {});
    });

    await test.step("filters that match nobody show the no-match notice", async () => {
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      await driver.settingsMembersSearchFill("zzz-no-such-member");
      await expect.poll(() => driver.settingsMembersNoMatchVisible(), { timeout: 15_000 }).toBe(true);
      await driver.settingsMembersSearchFill("");
      await expect.poll(() => driver.settingsMembersNoMatchVisible(), { timeout: 15_000 }).toBe(false);
    });

    await test.step("a failed role change toasts the server message and keeps the table", async () => {
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      await driver.settingsMembersRoleEditorOpen(mentionName);
      await driver.settingsMembersRoleEditorPick("Admin");
      await expect
        .poll(
          async () => {
            const toast = await driver.rulesLastToast();
            return toast ? `${toast.title} ${toast.message}` : "";
          },
          { timeout: 15_000 }
        )
        .toContain("You cannot assign a role equal to or higher than your own");
      expect((await serverProjectMemberByEmail(seed.workspaceSlug, seed.projectId, mentionEmail, session))?.role).toBe(
        PROJECT_ROLE_MEMBER
      );
      expect((await driver.settingsMembersRows()).map((r) => r.email)).toContain(mentionEmail);
    });

    // No doomed-removal step: submitting a removal for an out-of-band
    // deleted membership kills the page in the old app (a burst of 404s,
    // then the page closes with no navigation or crash event; NEWFRONT-406),
    // so the role-change step above carries the failure-toast coverage and
    // PSET-024 covers the removal dialog itself.

    await test.step("the invite dialog resets its rows after close", async () => {
      await driver.settingsMembersOpen(seed.workspaceSlug, seed.projectId);
      await driver.settingsMembersOpenInvite();
      await driver.settingsMembersInviteAddRow();
      expect(await driver.settingsMembersInviteRowCount()).toBe(2);
      await driver.settingsMembersInviteCancel();
      await driver.settingsMembersOpenInvite();
      // The reset lands just after close, so a fast reopen briefly shows
      // the stale rows before settling back to one.
      await expect.poll(() => driver.settingsMembersInviteRowCount(), { timeout: 15_000 }).toBe(1);
      await driver.settingsMembersInviteCancel();
    });
  }
);
