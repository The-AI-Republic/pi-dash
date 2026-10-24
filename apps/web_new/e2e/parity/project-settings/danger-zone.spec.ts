// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-389): the danger zone — archive from the
// General tab, restore from the project card, and delete with double typed
// confirmation. Archive/restore entries, card reads and the delete phrase
// fill reuse the sibling areas' proven driver methods (same controls, no
// fork); the confirm-modal text, the danger-zone delete entry and the
// modal-scoped name fill are this area's own. Rows: PSET-010 (archive),
// PSET-011 (restore), PSET-012 (delete).
//
// Observed behavior notes: the archive modal names the project and warns
// its contents archive with it while its items leave search; confirming
// archives, toasts, and lands on the project list; the archived project
// stays directly reachable but refuses updates. Restore mode explains the
// project becomes visible to members again (wired from the archived card,
// while the danger zone always opens archive mode). Delete needs the exact
// project name plus the exact phrase "delete my project" with the submit
// disabled until both match; deleting clears the caller's project
// favorites; the default project is refused server-side with a 400.
import { test, expect } from "../fixtures";
import {
  archivesProjectArchivedAt,
  createFavorite,
  listFavorites,
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverDeleteProjectRaw,
  serverPatchProject,
  serverProjectFetchStatus,
  serverProjects,
  serverSearchIssueNames,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["PSET-010"], "archive from the danger zone leaves nav, stays reachable, drops search, refuses updates"),
  { tag: specTags(["PSET-010"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 arch ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N389"),
      {},
      session
    );
    const issueName = `${tag} issue`;
    await serverCreateIssueFull(seed.workspaceSlug, projectId, issueName, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      expect(await serverSearchIssueNames(seed.workspaceSlug, projectId, tag, session)).toContain(issueName);

      await test.step("the confirm modal names the project and warns what archiving takes", async () => {
        // The new driver opens archive from the danger zone itself; the
        // shared card-menu path never navigates there (NEWFRONT-606).
        if (driver.target === "web_new") {
          await driver.settingsGeneralOpenArchive(seed.workspaceSlug, projectId);
        } else {
          await driver.openArchiveProjectDialog(seed.workspaceSlug, projectId);
        }
        await expect
          .poll(() => driver.settingsGeneralConfirmModalText(), { timeout: 30_000 })
          .toContain("will be archived");
        const body = await driver.settingsGeneralConfirmModalText();
        expect(body).toContain(`${tag} project`);
        expect(body).toContain("search");
      });

      await test.step("confirming archives, confirms, and lands on the project list", async () => {
        await driver.confirmArchive();
        // The archive write cascades (search leaves, lists rebuild), so
        // the success notice can land well after the default 5s poll
        // window on a loaded stack (NEWFRONT-606).
        await expect
          .poll(() => driver.isToastVisible("has been archived successfully"), { timeout: 30_000 })
          .toBe(true);
        await expect
          .poll(() => archivesProjectArchivedAt(seed.workspaceSlug, projectId, session), { timeout: 30_000 })
          .not.toBeNull();
        await expect.poll(() => driver.currentUrlPath(), { timeout: 30_000 }).toContain("/projects");
        await expect.poll(() => driver.visibleProjectCardNames(), { timeout: 60_000 }).not.toContain(`${tag} project`);
        // The seeded project row proves the sidebar hydrated before the
        // absence below is read.
        await expect.poll(() => driver.projectRowHref(seed.projectName), { timeout: 30_000 }).not.toBeNull();
        await expect.poll(() => driver.projectRowHref(`${tag} project`), { timeout: 30_000 }).toBeNull();
      });

      await test.step("the project stays directly reachable but leaves search and refuses updates", async () => {
        await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);
        expect(await driver.settingsGeneralFormVisible()).toBe(true);
        expect(await serverSearchIssueNames(seed.workspaceSlug, projectId, tag, session)).not.toContain(`${tag} issue`);
        await expect(
          serverPatchProject(seed.workspaceSlug, projectId, session, { description: `${tag} edit` })
        ).rejects.toThrow();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-011"], "restore from the card returns the project to the list, usable again"),
  { tag: specTags(["PSET-011"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 rest ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N389"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      // The new driver opens archive from the danger zone itself (see above).
      if (driver.target === "web_new") {
        await driver.settingsGeneralOpenArchive(seed.workspaceSlug, projectId);
      } else {
        await driver.openArchiveProjectDialog(seed.workspaceSlug, projectId);
      }
      await driver.confirmArchive();
      await expect
        .poll(() => archivesProjectArchivedAt(seed.workspaceSlug, projectId, session), { timeout: 30_000 })
        .not.toBeNull();

      await test.step("the restore dialog explains re-visibility to members", async () => {
        await driver.openArchivedProjects(seed.workspaceSlug);
        await driver.awaitProjectCard(`${tag} project`);
        await driver.clickCardRestore(`${tag} project`);
        await expect
          .poll(() => driver.settingsGeneralConfirmModalText(), { timeout: 30_000 })
          .toContain("Restoring a project");
        const body = await driver.settingsGeneralConfirmModalText();
        expect(body).toContain("visible");
      });

      await test.step("confirming restores: toast, back in the list and sidebar, updates accepted", async () => {
        await driver.confirmRestore();
        await expect.poll(() => driver.isToastVisible("in your projects")).toBe(true);
        await expect
          .poll(() => archivesProjectArchivedAt(seed.workspaceSlug, projectId, session), { timeout: 30_000 })
          .toBeNull();
        await expect.poll(() => driver.visibleProjectCardNames(), { timeout: 60_000 }).toContain(`${tag} project`);
        await expect.poll(() => driver.projectRowHref(`${tag} project`), { timeout: 30_000 }).not.toBeNull();
        await serverPatchProject(seed.workspaceSlug, projectId, session, { description: `${tag} restored` });
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-012"], "delete needs both typed confirmations, clears favorites, removes everything"),
  { tag: specTags(["PSET-012"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 del ${Date.now()}`;
    const name = `${tag} project`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      name,
      parityProjectIdentifier("N389"),
      {},
      session
    );
    await createFavorite(seed.workspaceSlug, session, {
      entity_type: "project",
      entity_identifier: projectId,
      project_id: projectId,
      name,
    });
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);

      await test.step("mismatched confirmations keep the submit disabled", async () => {
        await driver.settingsGeneralOpenDelete();
        expect(await driver.isDeleteSubmitDisabled()).toBe(true);
        await driver.settingsGeneralFillDeleteName(name);
        expect(await driver.isDeleteSubmitDisabled()).toBe(true);
        await driver.fillDeleteConfirmPhrase("delete my projec");
        expect(await driver.isDeleteSubmitDisabled()).toBe(true);
        await driver.fillDeleteConfirmPhrase("delete my project");
        expect(await driver.isDeleteSubmitDisabled()).toBe(false);
      });

      await test.step("confirming deletes, lands on the list, clears favorites, removes everything", async () => {
        await driver.submitDelete();
        await expect.poll(() => driver.currentUrlPath(), { timeout: 30_000 }).toContain("/projects");
        await expect
          .poll(() => serverProjectFetchStatus(seed.workspaceSlug, projectId, session), { timeout: 30_000 })
          .toMatchObject({ status: 404 });
        const favs = await listFavorites(seed.workspaceSlug, session);
        expect(favs.filter((fav) => fav.entity_identifier === projectId)).toEqual([]);
        const projects = await serverProjects(seed.workspaceSlug, session);
        expect(projects.map((project) => project.id)).not.toContain(projectId);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-012"], "the default project is protected from deletion server-side"),
  { tag: specTags(["PSET-012"]) },
  async ({ seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const projects = await serverProjects(seed.workspaceSlug, session);
    let defaultId: string | null = null;
    for (const project of projects) {
      const fetched = await serverProjectFetchStatus(seed.workspaceSlug, project.id, session);
      if (fetched.record?.["is_default"] === true) {
        defaultId = project.id;
        break;
      }
    }
    if (defaultId === null) {
      throw new Error("[parity] the seed carries no default project for the protection scenario.");
    }
    const attempt = await serverDeleteProjectRaw(seed.workspaceSlug, defaultId, session);
    expect(attempt.status).toBe(400);
    expect(attempt.body).toContain("Default project cannot be deleted");
    expect((await serverProjectFetchStatus(seed.workspaceSlug, defaultId, session)).status).toBe(200);
  }
);
