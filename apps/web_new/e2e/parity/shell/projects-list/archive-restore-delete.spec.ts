// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Parity scenarios (NEWFRONT-124): archive / restore / delete lifecycle.
// Rows: SHELL-037 (archiving hides from list with a warning; restoring
// returns it), SHELL-038 (archived cards collapse to a muted state with
// inline restore + delete for admins), SHELL-039 (deletion is destructive,
// double-gated, and reachable only from the archived state). Green on
// apps/web first.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  addProjectMembersViaApi,
  archiveProjectViaApi,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  projectByName,
  seatFreshMember,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ARCHIVE = ["SHELL-037"];
const ARCHIVED_CARD = ["SHELL-038"];
const DELETE = ["SHELL-039"];

test(
  specTitle(ARCHIVE, "archiving hides a project from the list and restoring returns it"),
  { tag: specTags(ARCHIVE) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-arch");
    const suffix = uniqueSuffixForProjects();
    const ws = await createWorkspaceForProjects(owner, { name: `Arch WS ${suffix}`, slug: `arch-${suffix}` });
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);
    const projId = await createProjectViaApi(owner, ws.slug, { name: "Archivable Project", identifier: "ARCH" });

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Archivable Project");

    await test.step("archiving warns, then the project leaves the default list", async () => {
      await driver.openArchiveProjectDialog(ws.slug, projId);
      await expect.poll(() => driver.archiveDialogBodyText()).toContain("archived");
      await driver.confirmArchive();
      await expect.poll(() => driver.isToastVisible("has been archived successfully")).toBe(true);
      await expect
        .poll(async () => (await projectByName(owner, ws.slug, "Archivable Project"))?.archived_at ?? null)
        .not.toBeNull();

      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await expect.poll(() => driver.visibleProjectCardNames()).not.toContain("Archivable Project");
    });

    await test.step("the archived project appears under the archive view and restores", async () => {
      await driver.openArchivedProjects(ws.slug);
      await driver.awaitProjectCard("Archivable Project");
      await driver.clickCardRestore("Archivable Project");
      await driver.confirmRestore();
      await expect.poll(() => driver.isToastVisible("in your projects")).toBe(true);
      await expect
        .poll(async () => (await projectByName(owner, ws.slug, "Archivable Project"))?.archived_at ?? null)
        .toBeNull();
    });

    await test.step("restoring returns to the default list with the card back", async () => {
      await expect.poll(() => driver.currentUrlPath()).not.toContain("/archives");
      await driver.awaitProjectCard("Archivable Project");
      await expect.poll(() => driver.visibleProjectCardNames()).toContain("Archivable Project");
    });
  }
);

test(
  specTitle(ARCHIVED_CARD, "archived cards show a muted state with admin restore and delete"),
  { tag: specTags(ARCHIVED_CARD) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-acard");
    const suffix = uniqueSuffixForProjects();
    const ws = await createWorkspaceForProjects(owner, { name: `ACard WS ${suffix}`, slug: `acard-${suffix}` });
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);
    const projId = await createProjectViaApi(owner, ws.slug, { name: "Muted Project", identifier: "MUTD" });
    await archiveProjectViaApi(owner, ws.slug, projId);

    await driver.openAuthenticated(`/${ws.slug}/projects/archives`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Muted Project");

    await test.step("the card shows the archived marker and admin actions", async () => {
      expect(await driver.cardShowsArchivedMarker("Muted Project")).toBe(true);
      expect(await driver.archivedCardHasAdminActions("Muted Project")).toBe(true);
    });

    await test.step("a guest sees the muted card with no admin actions", async () => {
      const guest = await seatFreshMember(owner, ws.slug, ROLE.GUEST, "parity-acard-guest");
      await markOnboardedForProjects(guest);
      await setLastWorkspaceForProjects(guest, ws.id);
      await addProjectMembersViaApi(owner, ws.slug, projId, [{ member_id: guest.userId, role: ROLE.GUEST }]);
      await driver.openAuthenticated(`/${ws.slug}/projects/archives`, browserSessionCookies(guest));
      await driver.awaitProjectCard("Muted Project");
      expect(await driver.cardShowsArchivedMarker("Muted Project")).toBe(true);
      expect(await driver.archivedCardHasAdminActions("Muted Project")).toBe(false);
    });
  }
);

test(
  specTitle(DELETE, "deletion is double-gated and reachable only from the archived state"),
  { tag: specTags(DELETE) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-del");
    const suffix = uniqueSuffixForProjects();
    const ws = await createWorkspaceForProjects(owner, { name: `Del WS ${suffix}`, slug: `del-${suffix}` });
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);
    // The workspace's default (first) project is delete-protected by the
    // server, so the doomed project is created second.
    await createProjectViaApi(owner, ws.slug, { name: "Keeper Project", identifier: "KEEP" });
    const projId = await createProjectViaApi(owner, ws.slug, { name: "Doomed Project", identifier: "DOOM" });
    await archiveProjectViaApi(owner, ws.slug, projId);

    await driver.openAuthenticated(`/${ws.slug}/projects/archives`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Doomed Project");

    await test.step("the destructive button stays disabled until both fields are correct", async () => {
      await driver.openDeleteProjectDialog("Doomed Project");
      await expect.poll(() => driver.isDeleteSubmitDisabled()).toBe(true);
      await driver.fillDeleteProjectName("Wrong Name");
      await driver.fillDeleteConfirmPhrase("delete my project");
      await expect.poll(() => driver.isDeleteSubmitDisabled()).toBe(true);
      await driver.fillDeleteProjectName("Doomed Project");
      await driver.fillDeleteConfirmPhrase("wrong phrase");
      await expect.poll(() => driver.isDeleteSubmitDisabled()).toBe(true);
    });

    await test.step("correct entries remove the project with a success notice", async () => {
      await driver.fillDeleteConfirmPhrase("delete my project");
      await expect.poll(() => driver.isDeleteSubmitDisabled()).toBe(false);
      await driver.submitDelete();
      await expect.poll(() => projectByName(owner, ws.slug, "Doomed Project")).toBeUndefined();
      await expect.poll(() => driver.isToastVisible("Project deleted successfully")).toBe(true);
    });
  }
);
