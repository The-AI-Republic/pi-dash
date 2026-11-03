// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-124): project card identity + favorite star,
// card routing, and contextual quick actions. Rows: SHELL-032 (card
// contents + favorite star, star only for admin/member), SHELL-033 (card
// routing: member into issues, outsider intercepted to join, archived never
// navigates), SHELL-034 (quick actions / context menu stay contextual).
// Target alignment (NEWFRONT-295 gate, main#523): cards carry no cover
// slot, so the contents step asserts its absence and the actions steps
// prove the entries on the header-row menu instead of the old cover
// hover actions. No oracle: the old app still renders covers.
import { test, expect } from "../../fixtures";
import {
  NETWORK,
  ROLE,
  addProjectMembersViaApi,
  archiveProjectViaApi,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  projectByName,
  projectMemberRole,
  seatFreshMember,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const CONTENTS = ["SHELL-032"];
const ROUTING = ["SHELL-033"];
const ACTIONS = ["SHELL-034"];

test(
  specTitle(CONTENTS, "cards show identity and a favorite star that persists"),
  { tag: specTags(CONTENTS) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-card");
    const suffix = uniqueSuffixForProjects();
    const wsName = `Card WS ${suffix}`;
    const ws = await createWorkspaceForProjects(owner, { name: wsName, slug: `card-${suffix}` });
    await drainWorkspaceSeed(owner, ws.slug, wsName);
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);
    await createProjectViaApi(owner, ws.slug, { name: "Secret Card", identifier: "SCRD", network: NETWORK.PRIVATE });

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Secret Card");

    await test.step("the card shows short code, private mark and a generated creation line", async () => {
      expect(await driver.cardShortCode("Secret Card")).toContain("SCRD");
      expect(await driver.cardHasPrivateMark("Secret Card")).toBe(true);
      expect(await driver.cardSubText("Secret Card")).toContain("Created on");
    });

    await test.step("an admin sees the star and starring persists on the server", async () => {
      expect(await driver.cardHasFavoriteStar("Secret Card")).toBe(true);
      expect(await driver.isFavoritesOpen()).toBe(false);
      await driver.clickFavoriteStar("Secret Card");
      await expect.poll(async () => (await projectByName(owner, ws.slug, "Secret Card"))?.is_favorite).toBe(true);
    });

    await test.step("the first star opens the favorites section", async () => {
      await expect.poll(() => driver.isFavoritesOpen()).toBe(true);
    });

    await test.step("a created card shows no cover, its logo and member stack with count", async () => {
      await driver.clickHeaderCreateButton();
      await expect.poll(() => driver.isCreateProjectDialogVisible()).toBe(true);
      await driver.fillCreateProjectName("Plain Card");
      await driver.submitCreateProject();
      await driver.awaitProjectCard("Plain Card");
      expect(await driver.cardHasCoverImage("Plain Card")).toBe(false);
      expect(await driver.cardHasLogo("Plain Card")).toBe(true);
      await expect.poll(() => driver.cardAvatarStack("Plain Card")).toHaveLength(1);

      const mateOne = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "parity-card-m1");
      const mateTwo = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "parity-card-m2");
      const mateThree = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "parity-card-m3");
      const plain = await projectByName(owner, ws.slug, "Plain Card");
      await addProjectMembersViaApi(owner, ws.slug, plain!.id, [
        { member_id: mateOne.userId, role: ROLE.MEMBER },
        { member_id: mateTwo.userId, role: ROLE.MEMBER },
        { member_id: mateThree.userId, role: ROLE.MEMBER },
      ]);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.awaitProjectCard("Plain Card");
      await expect.poll(() => driver.cardAvatarStack("Plain Card")).toEqual(expect.arrayContaining(["+2"]));
    });

    await test.step("a guest sees no favorite star", async () => {
      // Guests outside the project see an empty list, so seat the guest on
      // the project itself: the star follows the workspace role, and a
      // project-member guest still gets none.
      const guest = await seatFreshMember(owner, ws.slug, ROLE.GUEST, "parity-card-guest");
      await markOnboardedForProjects(guest);
      await setLastWorkspaceForProjects(guest, ws.id);
      const secret = await projectByName(owner, ws.slug, "Secret Card");
      await addProjectMembersViaApi(owner, ws.slug, secret!.id, [{ member_id: guest.userId, role: ROLE.GUEST }]);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(guest));
      await driver.awaitProjectCard("Secret Card");
      expect(await driver.cardHasFavoriteStar("Secret Card")).toBe(false);
    });
  }
);

test(
  specTitle(ROUTING, "cards route members to issues, intercept outsiders, and archived cards never navigate"),
  { tag: specTags(ROUTING) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-route");
    const suffix = uniqueSuffixForProjects();
    const wsName = `Route WS ${suffix}`;
    const ws = await createWorkspaceForProjects(owner, { name: wsName, slug: `route-${suffix}` });
    await drainWorkspaceSeed(owner, ws.slug, wsName);
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);
    const memberProjectId = await createProjectViaApi(owner, ws.slug, { name: "Owned Proj", identifier: "OWND" });

    await test.step("a member lands in the project issues view", async () => {
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.awaitProjectCard("Owned Proj");
      await driver.clickProjectCard("Owned Proj");
      await expect.poll(() => driver.currentUrlPath()).toContain(`/projects/${memberProjectId}/issues`);
    });

    await test.step("an outsider is intercepted with a join prompt", async () => {
      const outsider = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "parity-route-out");
      await markOnboardedForProjects(outsider);
      await setLastWorkspaceForProjects(outsider, ws.id);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(outsider));
      await driver.awaitProjectCard("Owned Proj");
      await driver.clickProjectCard("Owned Proj");
      await expect.poll(() => driver.isJoinDialogVisible()).toBe(true);
      expect(await driver.currentUrlPath()).toContain("/projects");
      expect(await driver.currentUrlPath()).not.toContain("/issues");
    });

    await test.step("an archived card does not navigate", async () => {
      await archiveProjectViaApi(owner, ws.slug, memberProjectId);
      await driver.openAuthenticated(`/${ws.slug}/projects/archives`, browserSessionCookies(owner));
      await driver.awaitProjectCard("Owned Proj");
      await driver.clickProjectCard("Owned Proj");
      expect(await driver.currentUrlPath()).toContain("/archives");
      expect(await driver.currentUrlPath()).not.toContain("/issues");
    });
  }
);

test(
  specTitle(ACTIONS, "card quick actions and context menu stay strictly contextual"),
  { tag: specTags(ACTIONS) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-actions");
    const suffix = uniqueSuffixForProjects();
    const wsName = `Actions WS ${suffix}`;
    const ws = await createWorkspaceForProjects(owner, { name: wsName, slug: `actions-${suffix}` });
    await drainWorkspaceSeed(owner, ws.slug, wsName);
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);
    const projId = await createProjectViaApi(owner, ws.slug, { name: "Owned Action", identifier: "OWNA" });

    await test.step("an admin on an active card sees settings and copy-link, not join", async () => {
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.awaitProjectCard("Owned Action");
      await driver.openCardContextMenu("Owned Action");
      const items = await driver.contextMenuItemLabels();
      expect(items).toEqual(expect.arrayContaining(["Settings", "Copy link"]));
      expect(items).not.toContain("Join");
      await driver.closeMenu();
    });

    await test.step("copy-link writes the project address to the clipboard", async () => {
      await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
      await driver.openCardContextMenu("Owned Action");
      await driver.clickCardContextMenuItem("Copy link");
      await expect.poll(() => driver.readClipboardText()).toContain(`/projects/${projId}/issues`);
    });

    await test.step("settings navigates to the project settings page", async () => {
      await driver.openCardContextMenu("Owned Action");
      await driver.clickCardContextMenuItem("Settings");
      await expect.poll(() => driver.currentUrlPath()).toContain(`/settings/projects/${projId}`);
    });

    await test.step("a non-member sees join and open-in-new-tab", async () => {
      const outsider = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "parity-actions-out");
      await markOnboardedForProjects(outsider);
      await setLastWorkspaceForProjects(outsider, ws.id);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(outsider));
      await driver.awaitProjectCard("Owned Action");
      await driver.openCardContextMenu("Owned Action");
      const items = await driver.contextMenuItemLabels();
      expect(items).toEqual(expect.arrayContaining(["Join", "Open in new tab"]));
      await driver.closeMenu();
    });

    await test.step("open-in-new-tab opens the project issues view", async () => {
      const outsider = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "parity-actions-tab");
      await markOnboardedForProjects(outsider);
      await setLastWorkspaceForProjects(outsider, ws.id);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(outsider));
      await driver.awaitProjectCard("Owned Action");
      await driver.openCardContextMenu("Owned Action");
      const popupPromise = driver.page.context().waitForEvent("page");
      await driver.clickCardContextMenuItem("Open in new tab");
      const popup = await popupPromise;
      await popup.waitForLoadState("domcontentloaded");
      expect(popup.url()).toContain(`/projects/${projId}/issues`);
      await popup.close();
    });

    await test.step("join opens the join dialog and confirming lands inside", async () => {
      const outsider = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "parity-actions-join");
      await markOnboardedForProjects(outsider);
      await setLastWorkspaceForProjects(outsider, ws.id);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(outsider));
      await driver.awaitProjectCard("Owned Action");
      await driver.openCardContextMenu("Owned Action");
      await driver.clickCardContextMenuItem("Join");
      await expect.poll(() => driver.isJoinDialogVisible()).toBe(true);
      await driver.confirmJoin();
      await expect.poll(() => driver.currentUrlPath()).toContain(`/projects/${projId}/issues`);
      await expect.poll(() => projectMemberRole(outsider, ws.slug, projId)).toBe(ROLE.MEMBER);
    });

    await test.step("an admin on an archived card sees restore and delete", async () => {
      await archiveProjectViaApi(owner, ws.slug, projId);
      const doomedId = await createProjectViaApi(owner, ws.slug, { name: "Doomed Action", identifier: "DMDA" });
      await archiveProjectViaApi(owner, ws.slug, doomedId);
      await driver.openAuthenticated(`/${ws.slug}/projects/archives`, browserSessionCookies(owner));
      await driver.awaitProjectCard("Owned Action");
      await driver.openCardContextMenu("Owned Action");
      const items = await driver.contextMenuItemLabels();
      expect(items).toEqual(expect.arrayContaining(["Restore", "Delete"]));
      await driver.closeMenu();
    });

    await test.step("restore opens its dialog and returns the project", async () => {
      await driver.openCardContextMenu("Owned Action");
      await driver.clickCardContextMenuItem("Restore");
      await expect.poll(() => driver.isRestoreDialogVisible()).toBe(true);
      await driver.confirmRestore();
      await expect
        .poll(async () => (await projectByName(owner, ws.slug, "Owned Action"))?.archived_at ?? null)
        .toBeNull();
    });

    await test.step("delete opens the destructive dialog", async () => {
      await driver.openAuthenticated(`/${ws.slug}/projects/archives`, browserSessionCookies(owner));
      await driver.awaitProjectCard("Doomed Action");
      await driver.openCardContextMenu("Doomed Action");
      await driver.clickCardContextMenuItem("Delete");
      await expect.poll(() => driver.isDeleteSubmitDisabled()).toBe(true);
    });

    await test.step("quick actions mount on the header row, never on a cover", async () => {
      // Target per main#523: no cover slot exists to mount on, and the
      // header-row menu carries the same contextual entries the steps
      // above proved entry by entry.
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.awaitProjectCard("Owned Action");
      expect(await driver.cardHasCoverImage("Owned Action")).toBe(false);
      await driver.openCardContextMenu("Owned Action");
      expect(await driver.contextMenuItemLabels()).toEqual(expect.arrayContaining(["Settings", "Copy link"]));
      await driver.closeMenu();
    });
  }
);
