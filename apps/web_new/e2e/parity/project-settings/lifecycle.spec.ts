// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-390): project lifecycle — create (PSET-013),
// restriction-screen join (PSET-014), leave ordering (PSET-015), publish
// modal (PSET-016), projects-list search locality (PSET-017). Green on
// apps/web through drivers/web first.
//
// Behavior notes from the old sources (not copied): the post-create
// feature-selection step was intentionally hidden upstream, so creation
// closes the modal with a success notice and shows no feature step; the
// restriction screen joins with a single action; leave navigates to the
// list before firing its write; join failures on the restriction screen
// only log (no notice), so no failure step is asserted there.
import { test, expect } from "../fixtures";
import {
  NETWORK,
  ROLE,
  addProjectMembersViaApi,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  deleteUiCreatedProject,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  projectByName,
  projectFeatureFlags,
  projectMemberRole,
  seatFreshMember,
  serverProjectStates,
  serverPublishBoardFacts,
  serverUnpublishBoard,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const CREATE = ["PSET-013"];
const JOIN = ["PSET-014"];
const LEAVE = ["PSET-015"];
const PUBLISH = ["PSET-016"];
const SEARCH = ["PSET-017"];

const POLL = { timeout: 60_000 };

async function freshOwnerWorkspace(prefix: string) {
  const owner = await signUpAuthedSession(prefix);
  const suffix = uniqueSuffixForProjects();
  const wsName = `${prefix} WS ${suffix}`;
  const ws = await createWorkspaceForProjects(owner, { name: wsName, slug: `${prefix}-${suffix}` });
  await drainWorkspaceSeed(owner, ws.slug, wsName);
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  return { owner, ws };
}

test(
  specTitle(CREATE, "create derives the identifier, seats the creator as admin, and closes"),
  { tag: specTags(CREATE) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("lifecycle-create");

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await expect.poll(() => driver.emptyStateHeading(), POLL).toBe("No active projects");
    await driver.clickEmptyStateCreate();
    await expect.poll(() => driver.isCreateProjectDialogVisible(), POLL).toBe(true);

    if (driver.target === "web_new") {
      // TARGET (main#503/main#523, NEWFRONT-606): the dialog shows the
      // AI-context placeholder and offers no cover input at all.
      await test.step("the dialog carries the AI-context placeholder and no cover input", async () => {
        expect(await driver.createFormDescriptionPlaceholder()).toBe("Project Context (Will be shown to AI agents)");
        expect(await driver.createFormCoverVisible()).toBe(false);
      });
    }

    await test.step("the identifier derives live until hand-edited", async () => {
      await driver.fillCreateProjectName("Lifecycle Forge");
      const derived = await driver.createProjectShortCodeValue();
      expect(derived.length).toBeGreaterThan(0);
      await driver.fillCreateProjectShortCode("LCFX");
      await driver.fillCreateProjectName("Lifecycle Forge Extended");
      expect(await driver.createProjectShortCodeValue()).toBe("LCFX");
    });

    await test.step("submitting closes the modal with a success notice and no feature step", async () => {
      await driver.submitCreateProject();
      await expect.poll(() => driver.isCreateProjectDialogVisible(), POLL).toBe(false);
      await expect.poll(() => driver.isToastVisible("Project created successfully"), POLL).toBe(true);
      expect(await driver.isCreateFeatureStepVisible()).toBe(false);
    });

    await test.step("the server stored defaults with the creator as admin", async () => {
      const created = await projectByName(owner, ws.slug, "Lifecycle Forge Extended");
      expect(created?.identifier).toBe("LCFX");
      expect(created?.member_role).toBe(ROLE.ADMIN);
      const flags = await projectFeatureFlags(owner, ws.slug, "Lifecycle Forge Extended");
      expect(flags?.network).toBe(NETWORK.PUBLIC);
      expect(flags?.cycle_view).toBe(false);
      expect(flags?.module_view).toBe(false);
      expect(flags?.issue_views_view).toBe(false);
      expect(flags?.page_view).toBe(false);
      expect(flags?.intake_view).toBe(false);
      const states = await serverProjectStates(ws.slug, created!.id, owner.cookie);
      expect(states.length).toBeGreaterThan(0);
    });

    await test.step("cleanup removes the created project", async () => {
      const created = await projectByName(owner, ws.slug, "Lifecycle Forge Extended");
      await deleteUiCreatedProject(owner, ws.slug, created!.id);
      await expect.poll(() => projectByName(owner, ws.slug, "Lifecycle Forge Extended"), POLL).toBeUndefined();
    });
  }
);

test(
  specTitle(JOIN, "a non-member joins from the restriction screen and lands on the board"),
  { tag: specTags(JOIN) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("lifecycle-join");
    const projId = await createProjectViaApi(owner, ws.slug, {
      name: "Joinable Lifecycle",
      identifier: "JLCY",
      network: NETWORK.PUBLIC,
    });

    const member = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "lifecycle-join-user");
    await markOnboardedForProjects(member);
    await setLastWorkspaceForProjects(member, ws.id);

    await driver.openAuthenticated(`/${ws.slug}/projects/${projId}/issues`, browserSessionCookies(member));

    await test.step("the restriction screen offers its single join action", async () => {
      await expect.poll(() => driver.isProjectRestrictionVisible(), POLL).toBe(true);
      expect(await driver.restrictionJoinLabel()).toBe("Join project");
    });

    await test.step("confirming lands on the issue board with membership recorded", async () => {
      // The screen joins in place (no route change): the restriction lifts
      // and the board renders at the same issues address.
      await driver.clickRestrictionJoin();
      await expect.poll(() => driver.isProjectRestrictionVisible(), POLL).toBe(false);
      await expect.poll(() => driver.currentUrlPath(), POLL).toContain(`/projects/${projId}/issues`);
      await expect.poll(() => projectMemberRole(member, ws.slug, projId), POLL).toBe(ROLE.MEMBER);
    });
  }
);

test(
  specTitle(LEAVE, "leave navigates to the list before firing the leave write"),
  { tag: specTags(LEAVE) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("lifecycle-leave");
    const projId = await createProjectViaApi(owner, ws.slug, {
      name: "Leavable Lifecycle",
      identifier: "LLCY",
      network: NETWORK.PUBLIC,
    });

    const guest = await seatFreshMember(owner, ws.slug, ROLE.GUEST, "lifecycle-leave-user");
    await markOnboardedForProjects(guest);
    await setLastWorkspaceForProjects(guest, ws.id);
    await addProjectMembersViaApi(owner, ws.slug, projId, [{ member_id: guest.userId, role: ROLE.GUEST }]);
    expect(await projectMemberRole(guest, ws.slug, projId)).toBe(ROLE.GUEST);

    await driver.openAuthenticated(`/${ws.slug}/projects/${projId}/issues`, browserSessionCookies(guest));
    await driver.openLeaveProjectDialog("Leavable Lifecycle");
    await expect.poll(() => driver.isLeaveDialogVisible(), POLL).toBe(true);

    await test.step("a wrong project name is refused with the membership intact", async () => {
      await driver.fillLeaveProjectName("Wrong Name");
      await driver.fillLeaveConfirmPhrase("Leave Project");
      await driver.submitLeave();
      await expect.poll(() => driver.leaveErrorText(), POLL).not.toBeNull();
      expect(await projectMemberRole(guest, ws.slug, projId)).toBe(ROLE.GUEST);
    });

    await test.step("a wrong confirmation phrase is refused with the membership intact", async () => {
      await driver.fillLeaveProjectName("Leavable Lifecycle");
      await driver.fillLeaveConfirmPhrase("leave");
      await driver.submitLeave();
      await expect.poll(() => driver.leaveErrorText(), POLL).not.toBeNull();
      expect(await projectMemberRole(guest, ws.slug, projId)).toBe(ROLE.GUEST);
    });

    await test.step("correct entries navigate to the list before the write lands", async () => {
      await driver.fillLeaveProjectName("Leavable Lifecycle");
      await driver.fillLeaveConfirmPhrase("Leave Project");
      // The write is held five seconds: the list route still lands at
      // once, with the membership intact until the held write completes.
      await driver.submitLeaveHoldingWrite(5_000);
      await expect.poll(() => driver.currentUrlPath(), POLL).toContain(`/${ws.slug}/projects`);
      expect(await projectMemberRole(guest, ws.slug, projId)).toBe(ROLE.GUEST);
      await expect.poll(() => projectMemberRole(guest, ws.slug, projId), POLL).toBeNull();
    });
  }
);

test(
  specTitle(PUBLISH, "publish modal loads, gates layouts, toggles, updates, and unpublishes"),
  { tag: specTags(PUBLISH) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("lifecycle-publish");
    const projId = await createProjectViaApi(owner, ws.slug, {
      name: "Publishable Lifecycle",
      identifier: "PLCY",
      network: NETWORK.PUBLIC,
    });

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.delayNextPublishRead(2_000);
    await driver.openPublishProjectModal(ws.slug, projId, "Publishable Lifecycle");

    await test.step("settings load behind a loader, then offer two layouts and three toggles", async () => {
      await expect.poll(() => driver.isPublishModalLoading(), POLL).toBe(true);
      await expect.poll(() => driver.isPublishModalLoading(), POLL).toBe(false);
      expect(await driver.publishLayoutOptions()).toEqual(["List", "Kanban"]);
      expect(await driver.publishSelectedLayouts()).toEqual(["List", "Kanban"]);
      expect(await driver.publishToggleState("comments")).toBe(false);
      expect(await driver.publishToggleState("reactions")).toBe(false);
      expect(await driver.publishToggleState("voting")).toBe(false);
      expect(await driver.isPublishUpdateVisible()).toBe(false);
      expect(await driver.isPublishUnpublishVisible()).toBe(false);
      expect(await driver.publishLiveLink()).toBeNull();
    });

    await test.step("the last layout cannot be deselected", async () => {
      await driver.publishToggleLayout("Kanban");
      expect(await driver.publishSelectedLayouts()).toEqual(["List"]);
      await driver.publishToggleLayout("List");
      expect(await driver.publishSelectedLayouts()).toEqual(["List"]);
      await driver.publishToggleLayout("Kanban");
      expect(await driver.publishSelectedLayouts()).toEqual(["List", "Kanban"]);
    });

    await test.step("publishing with mixed toggles reveals the live link", async () => {
      await driver.setPublishToggle("comments", true);
      await driver.setPublishToggle("voting", true);
      await driver.delayNextPublishWrite(1_500);
      await driver.submitPublish();
      await expect.poll(() => driver.publishLiveLink(), POLL).not.toBeNull();
      expect(await driver.publishLiveLinkTarget()).toBe("_blank");
      expect(await driver.isPublishUnpublishVisible()).toBe(true);
    });

    await test.step("copying the live link writes it to the clipboard", async () => {
      const link = await driver.publishLiveLink();
      await driver.clickPublishCopyLink();
      await expect.poll(() => driver.isToastVisible("Published page link copied successfully."), POLL).toBe(true);
      await expect.poll(() => driver.readClipboard(), POLL).toBe(link);
    });

    await test.step("the server stored the chosen layouts and toggles", async () => {
      const board = await serverPublishBoardFacts(ws.slug, projId, owner.cookie);
      expect(board?.is_comments_enabled).toBe(true);
      expect(board?.is_reactions_enabled).toBe(false);
      expect(board?.is_votes_enabled).toBe(true);
      expect(board?.view_props["list"]).toBe(true);
      expect(board?.view_props["kanban"]).toBe(true);
    });

    await test.step("updates stay hidden until dirty, then apply", async () => {
      expect(await driver.isPublishUpdateVisible()).toBe(false);
      await driver.setPublishToggle("voting", false);
      await expect.poll(() => driver.isPublishUpdateVisible(), POLL).toBe(true);
      await driver.submitPublishUpdate();
      await expect.poll(() => driver.isPublishModalVisible(), POLL).toBe(false);
      await expect
        .poll(async () => (await serverPublishBoardFacts(ws.slug, projId, owner.cookie))?.is_votes_enabled, POLL)
        .toBe(false);
    });

    await test.step("unpublishing carries its own progress and clears the board", async () => {
      await driver.openPublishProjectModal(ws.slug, projId, "Publishable Lifecycle");
      await expect.poll(() => driver.isPublishUnpublishVisible(), POLL).toBe(true);
      await driver.delayNextPublishWrite(2_000);
      await driver.clickPublishUnpublish();
      await expect.poll(() => driver.isPublishUnpublishing(), POLL).toBe(true);
      await expect.poll(() => driver.isPublishUnpublishVisible(), POLL).toBe(false);
      await expect.poll(() => serverPublishBoardFacts(ws.slug, projId, owner.cookie), POLL).toBeNull();
      const boardId = (await serverPublishBoardFacts(ws.slug, projId, owner.cookie))?.id;
      if (boardId) await serverUnpublishBoard(ws.slug, projId, boardId, owner.cookie);
    });

    await test.step("cleanup removes the published project", async () => {
      await deleteUiCreatedProject(owner, ws.slug, projId);
      await expect.poll(() => projectByName(owner, ws.slug, "Publishable Lifecycle"), POLL).toBeUndefined();
    });
  }
);

test(
  specTitle(SEARCH, "list search filters locally with no data requests"),
  { tag: specTags(SEARCH) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("lifecycle-search");
    await createProjectViaApi(owner, ws.slug, { name: "Findable Lifecycle", identifier: "FLCY" });
    await createProjectViaApi(owner, ws.slug, { name: "Hidden Lifecycle", identifier: "HLCY" });

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Findable Lifecycle");

    await test.step("typing narrows the grid without firing data requests", async () => {
      await driver.armListSearchRequestSpy();
      await driver.openListSearch();
      await expect.poll(() => driver.isListSearchExpanded(), POLL).toBe(true);
      await driver.typeListSearch("Findable");
      await expect.poll(() => driver.visibleProjectCardNames(), POLL).toEqual(["Findable Lifecycle"]);
      await expect.poll(() => driver.listSearchValue(), POLL).toBe("Findable");
      expect(await driver.listSearchRequestPaths()).toEqual([]);
    });

    await test.step("escape clears first, then collapses", async () => {
      await driver.pressEscapeInListSearch();
      await expect.poll(() => driver.listSearchValue(), POLL).toBe("");
      await driver.pressEscapeInListSearch();
      await expect.poll(() => driver.isListSearchExpanded(), POLL).toBe(false);
    });

    await test.step("an outside click collapses only when the field is empty", async () => {
      await driver.openListSearch();
      await expect.poll(() => driver.isListSearchExpanded(), POLL).toBe(true);
      await driver.clickOutsideListSearch();
      await expect.poll(() => driver.isListSearchExpanded(), POLL).toBe(false);
      await driver.openListSearch();
      await driver.typeListSearch("Findable");
      await expect.poll(() => driver.visibleProjectCardNames(), POLL).toEqual(["Findable Lifecycle"]);
      await driver.clickOutsideListSearch();
      await expect.poll(() => driver.isListSearchExpanded(), POLL).toBe(true);
    });
  }
);
