// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Parity scenarios (NEWFRONT-124): the project join and leave flows. Rows:
// SHELL-035 (non-member join confirms, shows joining, lands inside the
// project, card flips to member), SHELL-036 (leave demands two typed
// confirmations and warns about losing assigned work). Green on apps/web.
import { test, expect } from "../../fixtures";
import {
  NETWORK,
  ROLE,
  addProjectMembersViaApi,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  projectMemberRole,
  seatFreshMember,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const JOIN = ["SHELL-035"];
const LEAVE = ["SHELL-036"];

test(
  specTitle(JOIN, "a non-member joins a project and lands inside it"),
  { tag: specTags(JOIN) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-join");
    const suffix = uniqueSuffixForProjects();
    const ws = await createWorkspaceForProjects(owner, { name: `Join WS ${suffix}`, slug: `join-${suffix}` });
    await markOnboardedForProjects(owner);
    const projId = await createProjectViaApi(owner, ws.slug, {
      name: "Joinable Project",
      identifier: "JNBL",
      network: NETWORK.PUBLIC,
    });

    const member = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "parity-join-user");
    await markOnboardedForProjects(member);
    await setLastWorkspaceForProjects(member, ws.id);

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(member));
    await driver.awaitProjectCard("Joinable Project");

    await test.step("confirming the join dialog routes into the issues view", async () => {
      await driver.clickCardJoin("Joinable Project");
      await expect.poll(() => driver.joinDialogHeading()).toBe("Join Project?");
      await driver.confirmJoin();
      await expect.poll(() => driver.currentUrlPath()).toContain(`/projects/${projId}/issues`);
    });

    await test.step("the server records the new membership", async () => {
      await expect.poll(() => projectMemberRole(member, ws.slug, projId)).toBe(ROLE.MEMBER);
    });
  }
);

test(
  specTitle(LEAVE, "leaving a project demands two typed confirmations"),
  { tag: specTags(LEAVE) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-leave");
    const suffix = uniqueSuffixForProjects();
    const ws = await createWorkspaceForProjects(owner, { name: `Leave WS ${suffix}`, slug: `leave-${suffix}` });
    await markOnboardedForProjects(owner);
    const projId = await createProjectViaApi(owner, ws.slug, {
      name: "Leavable Project",
      identifier: "LVBL",
      network: NETWORK.PUBLIC,
    });

    // The Leave entry renders only for members without an admin/member
    // project role, so the leaver is a guest the owner seated directly.
    const guest = await seatFreshMember(owner, ws.slug, ROLE.GUEST, "parity-leave-user");
    await markOnboardedForProjects(guest);
    await setLastWorkspaceForProjects(guest, ws.id);
    await addProjectMembersViaApi(owner, ws.slug, projId, [{ member_id: guest.userId, role: ROLE.GUEST }]);
    expect(await projectMemberRole(guest, ws.slug, projId)).toBe(ROLE.GUEST);

    await driver.openAuthenticated(`/${ws.slug}/projects/${projId}/issues`, browserSessionCookies(guest));

    await test.step("a wrong project name is rejected with a corrective error", async () => {
      await driver.openLeaveProjectDialog("Leavable Project");
      await expect.poll(() => driver.isLeaveDialogVisible()).toBe(true);
      await driver.fillLeaveProjectName("Wrong Name");
      await driver.fillLeaveConfirmPhrase("Leave Project");
      await driver.submitLeave();
      await expect.poll(() => driver.leaveErrorText()).not.toBeNull();
      expect(await projectMemberRole(guest, ws.slug, projId)).toBe(ROLE.GUEST);
    });

    await test.step("correct entries remove membership and route back to the list", async () => {
      await driver.fillLeaveProjectName("Leavable Project");
      await driver.fillLeaveConfirmPhrase("Leave Project");
      await driver.submitLeave();
      await expect.poll(() => projectMemberRole(guest, ws.slug, projId)).toBeNull();
      await expect.poll(() => driver.currentUrlPath()).toContain(`/${ws.slug}/projects`);
    });
  }
);
