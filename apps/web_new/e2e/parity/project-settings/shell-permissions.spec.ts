// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-388): project-settings shell, navigation,
// permissions, pinned non-behaviors, edition parity. Rows PSET-001 (shell
// with grouped role-filtered sidebar, active highlight, narrow-screen
// drawer), PSET-002 (bare-path forward vs no-project guidance),
// PSET-003 (deep links, restriction with join, dirty-form needs no
// confirm), PSET-004 (role matrix plus server refusals), PSET-062
// (pinned absences), PSET-063 (same behavior in this OSS checkout, no
// edition-only additions).
//
// Observed behavior notes (the inventory rows carry them at update
// time): the sidebar groups tabs under General, Features, Work structure
// and Execution and hides whole groups a role cannot use (no disabled
// placeholders); the current tab carries the selected highlight on
// slash-form addresses, while the equivalent no-slash deep link renders
// with nothing marked (bug NEWFRONT-396); narrow screens swap the fixed
// sidebar for a drawer trigger; the bare settings/projects path forwards
// to the first joined project while a member of nothing sees a guidance
// view whose action opens creation; unknown ids render the "Project not
// found" restriction without a join action while a non-member opening a
// real project gets the join action and lands back on the same settings
// page authorized; leaving a dirty general form for another tab
// navigates with no confirm dialog; guests see General plus Members in
// the sidebar but the members page itself refuses them beside the kept
// sidebar; members additionally see States and Labels; the repository tab
// hides from members in the sidebar yet opens by direct address (page
// gate is wider than the sidebar filter); admin-only tabs refuse
// everyone else with the not-authorized view; out-of-role writes fail
// server-side while guest member-list reads stay allowed (the members
// page gate is UI-only); settings tabs register no shortcuts, drag
// targets, bulk,
// export or import controls and open no sockets (the only poller in the
// area is the scheduler run-history refresh, untouched here).
import { test, expect } from "../fixtures";
import {
  parityApiBase,
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCleanupWorkspaceMember,
  serverCompleteOnboarding,
  serverCreateProjectWithFlags,
  serverProjectMembers,
  serverProjects,
  serverProvisionWorkspaceMember,
  serverRequestStatus,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Full admin sidebar in display order: every tab the registry defines. */
const ADMIN_ITEMS = [
  "General",
  "Members",
  "Cycles",
  "Modules",
  "Views",
  "Pages",
  "Intake",
  "States",
  "Labels",
  "Estimates",
  "Automations",
  "Repository",
  "Schedulers",
];

/**
 * The new app additionally ships the new-app-first AI Workers tab last
 * (PSET-064, NEWFRONT-387); members see it too (visibleTo member), so
 * their sidebar gains both the entry and its Execution group.
 */
const ADMIN_ITEMS_NEW = [...ADMIN_ITEMS, "AI Workers"];
const MEMBER_ITEMS_NEW = ["General", "Members", "States", "Labels", "AI Workers"];
const MEMBER_GROUPS_NEW = ["General", "Work structure", "Execution"];

/** Seeded guest identity (project guest, role 5); throws when the seed lacks it. */
function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Provision a workspace member with onboarding completed, for UI sign-in. */
async function provisionMember(
  seed: ParitySeedFacts,
  ownerSession: string,
  prefix: string
): Promise<{ userId: string; session: string; email: string; password: string }> {
  const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, prefix, ownerSession);
  await serverCompleteOnboarding(member.session);
  return member;
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

/** Workspace membership id for cleanup, resolved by provisioned user id. */
async function membershipIdFor(seed: ParitySeedFacts, ownerSession: string, userId: string): Promise<string> {
  const rows = await serverWorkspaceMembers(seed.workspaceSlug, ownerSession);
  const row = rows.find((entry) => entry.userId === userId);
  if (!row) throw new Error("[parity] provisioned member missing from workspace list.");
  return row.membershipId;
}

test(
  specTitle(["PSET-001"], "settings shell groups role-filtered tabs, highlights the tab, draws on narrow screens"),
  { tag: specTags(["PSET-001"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

    await test.step("admin sees all four groups with every tab", async () => {
      // Slash-form address, the form the sidebar links themselves use;
      // the no-slash form renders identically but marks nothing active
      // (bug: NEWFRONT-396, pinned by the next scenario).
      await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, "/");
      expect(await driver.settingsShellGroups()).toEqual(["General", "Features", "Work structure", "Execution"]);
      expect(await driver.settingsShellItems()).toEqual(driver.target === "web_new" ? ADMIN_ITEMS_NEW : ADMIN_ITEMS);
      expect(await driver.settingsShellActiveItem()).toBe("General");
      const facts = await serverProjects(seed.workspaceSlug, session);
      expect(facts.some((row) => row.id === seed.projectId)).toBe(true);
    });

    await test.step("following sidebar items moves the highlight", async () => {
      await driver.settingsShellFollowItem("Members");
      expect(await driver.settingsShellPath()).toBe(
        `/${seed.workspaceSlug}/settings/projects/${seed.projectId}/members`
      );
      // The rail re-renders on the new pathname after the navigation settles;
      // poll so a loaded shared runner cannot read the previous marker.
      await expect.poll(() => driver.settingsShellActiveItem()).toBe("Members");
      await driver.settingsShellFollowItem("States");
      await expect.poll(() => driver.settingsShellActiveItem()).toBe("States");
    });

    await test.step("narrow screens swap the sidebar for a drawer with the same tabs", async () => {
      try {
        expect(await driver.settingsShellDrawerTriggerVisible()).toBe(true);
        await driver.settingsShellOpenDrawer();
        const drawer = await driver.settingsShellDrawerItems();
        for (const item of driver.target === "web_new" ? ADMIN_ITEMS_NEW : ADMIN_ITEMS) expect(drawer).toContain(item);
      } finally {
        await driver.settingsShellWideViewport();
      }
    });
  }
);

test(
  specTitle(["PSET-001"], "bug: NEWFRONT-396 no-slash deep links render with no sidebar highlight"),
  { tag: specTags(["PSET-001"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    // The tabs render at no-slash addresses (see PSET-003) but the
    // sidebar marks nothing active: the old active check requires the
    // trailing slash. Intended: the current tab highlights in both forms.
    for (const href of ["", "/members"]) {
      await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, href);
      expect(await driver.settingsShellDeniedVisible()).toBe(false);
      expect(await driver.settingsShellRestrictionVisible()).toBe(false);
      expect(await driver.settingsShellActiveItem()).toBe(null);
    }
    const facts = await serverProjects(seed.workspaceSlug, session);
    expect(facts.some((row) => row.id === seed.projectId)).toBe(true);
  }
);

test(
  specTitle(["PSET-002"], "bare projects path forwards to the first joined project or guides with a create entry"),
  { tag: specTags(["PSET-002"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const joined = await serverProjects(seed.workspaceSlug, session);
    if (joined.length === 0) throw new Error("[parity] seed admin belongs to no project.");

    await test.step("a member forwards to the first joined project", async () => {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsProjectsOpen(seed.workspaceSlug);
      await expect
        .poll(() => driver.settingsShellPath(), { timeout: 60_000 })
        .toBe(`/${seed.workspaceSlug}/settings/projects/${joined[0]?.id}`);
      expect(await driver.settingsShellGroups()).toContain("General");
    });

    await test.step("a member of nothing sees guidance with a create entry", async () => {
      const outsider = await provisionMember(seed, session, "nf388-noproj");
      try {
        await switchUser(driver, outsider.email, outsider.password, seed.workspaceSlug);
        await driver.settingsProjectsOpen(seed.workspaceSlug);
        expect(await driver.settingsShellNoProjectsVisible()).toBe(true);
        await driver.settingsShellOpenGuidanceCreate();
        expect(await driver.isCreateProjectDialogVisible()).toBe(true);
      } finally {
        await serverCleanupWorkspaceMember(
          seed.workspaceSlug,
          await membershipIdFor(seed, session, outsider.userId),
          session
        );
      }
    });
  }
);

test(
  specTitle(["PSET-003"], "every tab deep-links; bad ids restrict with join, dirty forms leave without confirm"),
  { tag: specTags(["PSET-003"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

    await test.step("each tab renders at its own address", async () => {
      for (const href of ["", "/members", "/states", "/labels", "/estimates", "/github", "/schedulers"]) {
        await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, href);
        expect(await driver.settingsShellDeniedVisible()).toBe(false);
        expect(await driver.settingsShellRestrictionVisible()).toBe(false);
        expect(await driver.settingsShellPath()).toBe(
          `/${seed.workspaceSlug}/settings/projects/${seed.projectId}${href}`
        );
      }
    });

    await test.step("an unknown id restricts without a join action", async () => {
      await driver.settingsShellOpen(seed.workspaceSlug, "00000000-0000-0000-0000-000000000000", "");
      expect(await driver.settingsShellRestrictionVisible()).toBe(true);
      expect(await driver.settingsShellJoinVisible()).toBe(false);
      const restrictionText = await driver.settingsShellRestrictionText();
      expect(restrictionText).toContain("Project not found");
    });

    await test.step("a non-member joins and lands back on settings authorized", async () => {
      const tag = `NF388 join ${Date.now()}`;
      const projectId = await serverCreateProjectWithFlags(
        seed.workspaceSlug,
        `${tag} project`,
        parityProjectIdentifier("N388"),
        {},
        session
      );
      const newcomer = await provisionMember(seed, session, "nf388-join");
      try {
        await switchUser(driver, newcomer.email, newcomer.password, seed.workspaceSlug);
        await driver.settingsShellOpen(seed.workspaceSlug, projectId, "");
        expect(await driver.settingsShellRestrictionVisible()).toBe(true);
        expect(await driver.settingsShellJoinVisible()).toBe(true);
        await driver.settingsShellJoin();
        expect(await driver.settingsShellRestrictionVisible()).toBe(false);
        expect(await driver.settingsShellGeneralFormVisible()).toBe(true);
        const members = await serverProjectMembers(seed.workspaceSlug, projectId, session);
        expect(members.some((row) => row.userId === newcomer.userId)).toBe(true);
      } finally {
        await serverCleanupWorkspaceMember(
          seed.workspaceSlug,
          await membershipIdFor(seed, session, newcomer.userId),
          session
        );
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    });

    await test.step("leaving a dirty general form needs no confirm", async () => {
      await switchUser(driver, seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, "");
      expect(await driver.settingsShellGeneralFormVisible()).toBe(true);
      await driver.settingsShellFillGeneralName(`dirty ${Date.now()}`);
      await driver.settingsShellFollowItem("Members");
      expect(await driver.settingsShellPath()).toBe(
        `/${seed.workspaceSlug}/settings/projects/${seed.projectId}/members`
      );
    });
  }
);

test(
  specTitle(["PSET-004"], "role matrix gates tabs in UI and the server refuses out-of-role writes"),
  { tag: specTags(["PSET-004"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const guest = requireGuest(seed);
    const member = await provisionMember(seed, session, "nf388-member");
    await serverAddProjectMembers(seed.workspaceSlug, seed.projectId, [{ memberId: member.userId, role: 15 }], session);
    try {
      await test.step("guests see general plus members, and are refused beyond general", async () => {
        await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);
        await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, "");
        expect(await driver.settingsShellItems()).toEqual(["General", "Members"]);
        expect(await driver.settingsShellGeneralFormVisible()).toBe(true);
        await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, "/members");
        expect(await driver.settingsShellDeniedVisible()).toBe(true);
        await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, "/states");
        expect(await driver.settingsShellDeniedVisible()).toBe(true);
      });

      await test.step("members gain states and labels but not the admin tabs", async () => {
        await switchUser(driver, member.email, member.password, seed.workspaceSlug);
        await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, "");
        expect(await driver.settingsShellGroups()).toEqual(
          driver.target === "web_new" ? MEMBER_GROUPS_NEW : ["General", "Work structure"]
        );
        expect(await driver.settingsShellItems()).toEqual(
          driver.target === "web_new" ? MEMBER_ITEMS_NEW : ["General", "Members", "States", "Labels"]
        );
        for (const href of ["/estimates", "/automations", "/schedulers", "/features/cycles"]) {
          await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, href);
          expect(await driver.settingsShellDeniedVisible()).toBe(true);
        }
      });

      await test.step("the repository tab opens by direct address for members", async () => {
        await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, "/github");
        expect(await driver.settingsShellDeniedVisible()).toBe(false);
        expect(await driver.settingsShellRestrictionVisible()).toBe(false);
      });

      await test.step("the server refuses out-of-role writes", async () => {
        const apiBase = parityApiBase();
        const guestSession = await signInSession(guest.email, guest.password);
        const projectUrl = `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/`;
        const guestPatch = await serverRequestStatus("PATCH", projectUrl, guestSession, { name: "guest rename" });
        expect([401, 403]).toContain(guestPatch.status);
        const memberPatch = await serverRequestStatus("PATCH", projectUrl, member.session, { name: "member rename" });
        expect([401, 403]).toContain(memberPatch.status);
        const membersUrl = `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/members/`;
        const guestAdd = await serverRequestStatus("POST", membersUrl, guestSession, {
          members: [{ member_id: member.userId, role: 15 }],
        });
        expect([401, 403]).toContain(guestAdd.status);
        // Reads stay open where writes are refused: a guest may list
        // members through the API while the members page itself denies
        // them, so that page gate is UI-only.
        const guestList = await serverRequestStatus("GET", membersUrl, guestSession);
        expect(guestList.status).toBe(200);
        const facts = await serverProjects(seed.workspaceSlug, session);
        expect(facts.some((row) => row.id === seed.projectId)).toBe(true);
      });
    } finally {
      await serverCleanupWorkspaceMember(
        seed.workspaceSlug,
        await membershipIdFor(seed, session, member.userId),
        session
      );
    }
  }
);

test(
  specTitle(["PSET-062"], "settings pins its non-behaviors: no shortcuts, drag, bulk, export, palette tabs or sockets"),
  { tag: specTags(["PSET-062"]) },
  async ({ driver, seed }) => {
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, "");

    await test.step("typing inert keys changes nothing and raises no dialog", async () => {
      const before = await driver.settingsShellPath();
      const result = await driver.settingsShellPressKeys(["c", "e", "/", "?"]);
      expect(result.dialogSeen).toBe(false);
      expect(result.path).toBe(before);
    });

    await test.step("no bulk, export or import controls render on settings tabs", async () => {
      for (const href of ["", "/members", "/labels"]) {
        await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, href);
        expect(await driver.settingsShellBulkExportImportVisible()).toBe(false);
      }
    });

    await test.step("the palette offers only the two global settings shortcuts", async () => {
      await driver.pressPaletteOpenChord();
      expect(await driver.isCommandPaletteOpen()).toBe(true);
      await driver.typeInCommandPalette("project settings");
      const titles = await driver.paletteCommandTitles();
      const settingsEntries = titles.filter((title) => /setting/i.test(title));
      // The palette contributes no per-tab settings commands: the only
      // settings entries are the global navigation shortcuts.
      expect(settingsEntries.length).toBeLessThanOrEqual(2);
      await driver.closeCommandPaletteViaBackdrop();
    });

    await test.step("visiting tabs opens no realtime sockets", async () => {
      const sockets = await driver.settingsShellCountSockets(
        ["", "/members", "/states", "/labels"],
        seed.workspaceSlug,
        seed.projectId
      );
      expect(sockets).toBe(0);
    });
  }
);

test(
  specTitle(["PSET-063"], "this build offers the full settings matrix with no edition-only additions"),
  { tag: specTags(["PSET-063"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

    await test.step("every tab renders with no edition gate", async () => {
      await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, "");
      expect(await driver.settingsShellItems()).toEqual(driver.target === "web_new" ? ADMIN_ITEMS_NEW : ADMIN_ITEMS);
      const hrefs = [
        "",
        "/members",
        "/states",
        "/labels",
        "/estimates",
        "/automations",
        "/github",
        "/schedulers",
        "/features/cycles",
        "/features/modules",
        "/features/views",
        "/features/pages",
        "/features/intake",
      ];
      // The new-app-first AI Workers tab joins the matrix on web_new only.
      if (driver.target === "web_new") hrefs.push("/ai-workers");
      for (const href of hrefs) {
        await driver.settingsShellOpen(seed.workspaceSlug, seed.projectId, href);
        expect(await driver.settingsShellDeniedVisible()).toBe(false);
        expect(await driver.settingsShellEditionGateVisible()).toBe(false);
      }
    });

    await test.step("the same project API backs every build", async () => {
      const projects = await serverProjects(seed.workspaceSlug, session);
      expect(projects.some((row) => row.id === seed.projectId)).toBe(true);
    });
  }
);
