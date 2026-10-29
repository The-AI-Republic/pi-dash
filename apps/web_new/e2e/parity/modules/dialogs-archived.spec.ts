// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-309): modules dialogs, archive and archived
// screen. Rows MOD-022 (create dialog), MOD-023 (create outcome), MOD-024
// (edit dialog), MOD-025 (delete with confirmation), MOD-026 (archive with
// confirmation), MOD-027 (archived-modules screen), MOD-028 (restore).
// Green on apps/web first.
//
// The live archive path (POST .../modules/{id}/archive/) and the live
// restore path (DELETE .../modules/{id}/archive/) were proved by
// NEWFRONT-225; this spec reuses its archives* API helpers and drivers
// read-only and asserts MOD-026..028 from the modules angle without
// re-proving ARCH-020..025.
//
// Fixture shape: every scenario mints its own owner, workspace, and
// project (module view on), because display choices persist per project.
// Modules are created and archived through the API in-spec; the seed
// carries none.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  ROLE,
  addProjectMembersViaApi,
  archivesArchiveModule,
  archivesArchivedModules,
  archivesModuleIsArchived,
  archivesPatchModule,
  browserSessionCookies,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  modulesModuleDetail,
  modulesModuleGone,
  seatFreshMember,
  serverCreateModule,
  serverCreateProjectWithFlags,
  serverProjectModules,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_022 = ["MOD-022"];
const ROW_023 = ["MOD-023"];
const ROW_024 = ["MOD-024"];
const ROW_025 = ["MOD-025"];
const ROW_026 = ["MOD-026"];
const ROW_027 = ["MOD-027"];
const ROW_028 = ["MOD-028"];

interface ModulesWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  projectId: string;
}

/** Fresh owner, workspace, and modules-enabled project for one scenario. */
async function setupModulesWorld(prefix: string): Promise<ModulesWorld> {
  const owner = await signUpAuthedSession(prefix);
  // The project create 400s intermittently on a loaded scratch stack;
  // one retry with a fresh suffix rides it out, and a systematic
  // failure still fails twice.
  for (let attempt = 1; ; attempt++) {
    const suffix = uniqueSuffixForProjects();
    try {
      const ws = await createWorkspaceForProjects(owner, {
        name: `ModDlg WS ${suffix}`,
        slug: `modd-${suffix}`,
      });
      await markOnboardedForProjects(owner);
      await setLastWorkspaceForProjects(owner, ws.id);
      const projectId = await serverCreateProjectWithFlags(
        ws.slug,
        "Module Dialogs",
        `MD${suffix.slice(-8)}`.toUpperCase(),
        { moduleView: true },
        owner.cookie
      );
      return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, projectId };
    } catch (error) {
      if (attempt >= 2) throw error;
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
  }
}

/** Open the live modules list as `session`; resolves once rows settle. */
async function enterModulesList(driver: ParityDriver, world: ModulesWorld, session: AuthedSession): Promise<void> {
  await driver.openAuthenticated(
    `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
    browserSessionCookies(session)
  );
  await driver.archivesOpenLiveModules(world.workspaceSlug, world.projectId);
}

/**
 * Enter the archived-modules tab authenticated as `session`. A direct
 * load hangs on the skeleton (NEWFRONT-228), so entry always goes
 * through the live screen plus client-side navigation.
 */
async function enterArchivesModules(driver: ParityDriver, world: ModulesWorld, session: AuthedSession): Promise<void> {
  await driver.openAuthenticated(
    `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
    browserSessionCookies(session)
  );
  await driver.archivesOpenModulesTab(world.workspaceSlug, world.projectId);
}

/** Create a module and move it to `status`; resolves its id. */
async function createModuleWithStatus(
  world: ModulesWorld,
  name: string,
  status: string,
  session: AuthedSession = world.owner
): Promise<string> {
  const id = await serverCreateModule(world.workspaceSlug, world.projectId, name, session.cookie);
  await archivesPatchModule(world.workspaceSlug, world.projectId, id, { status }, session.cookie);
  return id;
}

test(
  specTitle(ROW_022, "create dialog shows the project picker, defaults, and tab order"),
  { tag: specTags(ROW_022) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod022");
    const suffix = uniqueSuffixForProjects();
    const allowedName = "Picker Allowed";
    const deniedName = "Picker Denied";
    const allowedId = await serverCreateProjectWithFlags(
      world.workspaceSlug,
      allowedName,
      `PA${suffix.slice(-8)}`.toUpperCase(),
      { moduleView: true },
      world.owner.cookie
    );
    const deniedId = await serverCreateProjectWithFlags(
      world.workspaceSlug,
      deniedName,
      `PD${suffix.slice(-8)}`.toUpperCase(),
      { moduleView: true },
      world.owner.cookie
    );
    const member = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.MEMBER, "parity-mod022m");
    await markOnboardedForProjects(member);
    await setLastWorkspaceForProjects(member, world.workspaceId);
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, allowedId, [
      { member_id: member.userId, role: ROLE.MEMBER },
    ]);
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, deniedId, [
      { member_id: member.userId, role: ROLE.GUEST },
    ]);
    const allowedWorld: ModulesWorld = { ...world, projectId: allowedId };

    await enterModulesList(driver, allowedWorld, member);
    await driver.modulesOpenCreateFromHeader();

    await test.step("the creation dialog carries a project picker", async () => {
      expect(await driver.modulesDialogHeading()).toBe("Create module");
      expect(await driver.modulesDialogHasProjectPicker()).toBe(true);
    });

    await test.step("the picker lists creation-permission projects only", async () => {
      const options = await driver.modulesDialogProjectOptions();
      expect(options).toContain(allowedName);
      expect(options).not.toContain(deniedName);
    });

    await test.step("fields start from defaults", async () => {
      expect(await driver.modulesDialogTitle()).toBe("");
      expect(await driver.modulesDialogDescription()).toBe("");
      expect(await driver.modulesDialogStatus()).toContain("Backlog");
      expect(await driver.modulesDialogLead()).toBe("Lead");
      expect(await driver.modulesDialogMembers()).toBe("Members");
      const range = await driver.modulesDialogDateRange();
      expect(range).toContain("Start date");
      expect(range).toContain("End date");
    });

    await driver.modulesDialogCancel();
  }
);

test(
  specTitle(ROW_022, "bug: NEWFRONT-330 the date control carries no tab index"),
  { tag: specTags(ROW_022) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod022t");
    await enterModulesList(driver, world, world.owner);
    await driver.modulesOpenCreateFromHeader();

    // The row requires a logical tab order; the old date-range control
    // renders no tabindex (NEWFRONT-330), so focus reaches it after
    // Submit instead of third. Every other control matches the
    // intended 1..8 sequence.
    if (driver.target === "web_new") {
      // Fixed (NEWFRONT-330): the new date control carries its tab
      // index, so focus reaches it third.
      expect(await driver.modulesDialogTabOrder()).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    } else {
      expect(await driver.modulesDialogTabOrder()).toEqual([1, 2, null, 4, 5, 6, 7, 8]);
    }
    await driver.modulesDialogCancel();
  }
);

test(
  specTitle(ROW_022, "create dialog title validation rejects empty and over-long titles"),
  { tag: specTags(ROW_022) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod022v");
    await enterModulesList(driver, world, world.owner);
    await driver.modulesOpenCreateFromEmpty();

    await test.step("an empty title is refused inline", async () => {
      expect(await driver.modulesDialogTitleError()).toBeNull();
      await driver.modulesDialogSubmitCreate();
      expect(await driver.modulesDialogOpen()).toBe(true);
      expect(await driver.modulesDialogTitleError()).toContain("required");
      expect(await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).toEqual([]);
    });

    await test.step("a 256-character title is refused inline", async () => {
      await driver.modulesDialogFillTitle("x".repeat(256));
      await driver.modulesDialogSubmitCreate();
      expect(await driver.modulesDialogOpen()).toBe(true);
      expect(await driver.modulesDialogTitleError()).toContain("255");
      expect(await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).toEqual([]);
    });

    await test.step("a valid title clears the error", async () => {
      await driver.modulesDialogFillTitle("Validation Passes");
      await driver.modulesDialogSubmitCreate();
      // Toast first (it expires), then the close (React unmounts behind it).
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Success!" });
      await expect.poll(() => driver.modulesDialogOpen(), { timeout: 30_000 }).toBe(false);
      const live = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      expect(live.map((row) => row.name)).toContain("Validation Passes");
    });
  }
);

test(
  specTitle(ROW_022, "cancel and escape close without saving; submit resets the form"),
  { tag: specTags(ROW_022) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod022c");
    await enterModulesList(driver, world, world.owner);

    await test.step("cancel closes without saving", async () => {
      await driver.modulesOpenCreateFromHeader();
      await driver.modulesDialogFillTitle("Unsaved Module");
      await driver.modulesDialogFillDescription("Unsaved description");
      await driver.modulesDialogCancel();
      expect(await driver.modulesDialogOpen()).toBe(false);
      expect(await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).toEqual([]);
    });

    await test.step("escape closes without saving", async () => {
      await driver.modulesOpenCreateFromHeader();
      await driver.modulesDialogFillTitle("Escaped Module");
      await driver.modulesDialogEscape();
      expect(await driver.modulesDialogOpen()).toBe(false);
      expect(await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).toEqual([]);
    });

    await test.step("a successful submit resets the form", async () => {
      await driver.modulesOpenCreateFromHeader();
      await driver.modulesDialogFillTitle("Reset Probe");
      await driver.modulesDialogFillDescription("Reset description");
      await driver.modulesDialogSubmitCreate();
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Success!" });
      await expect.poll(() => driver.modulesDialogOpen(), { timeout: 30_000 }).toBe(false);
      await driver.modulesOpenCreateFromHeader();
      expect(await driver.modulesDialogTitle()).toBe("");
      expect(await driver.modulesDialogDescription()).toBe("");
      expect(await driver.modulesDialogStatus()).toContain("Backlog");
      expect(await driver.modulesDialogLead()).toBe("Lead");
      expect(await driver.modulesDialogMembers()).toBe("Members");
      await driver.modulesDialogCancel();
    });
  }
);

test(
  specTitle(ROW_023, "successful creation closes the dialog and stores every field"),
  { tag: specTags(ROW_023) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod023");
    const member = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.MEMBER, "parity-mod023m");
    await markOnboardedForProjects(member);
    await setLastWorkspaceForProjects(member, world.workspaceId);
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
      { member_id: member.userId, role: ROLE.MEMBER },
    ]);

    await enterModulesList(driver, world, world.owner);
    await driver.modulesOpenCreateFromHeader();

    // The lead and members pickers list the same project membership, so
    // the lead labels name the member option for both controls.
    const leadLabels = await driver.modulesDialogLeadOptions();
    const memberLabel = leadLabels.find((label) => label !== "You") ?? "";
    expect(memberLabel).not.toBe("");

    await driver.modulesDialogFillTitle("Full Module");
    await driver.modulesDialogFillDescription("Line one\nLine two");
    await driver.modulesDialogPickStatus("Planned");
    await driver.modulesDialogPickLead("You");
    await driver.modulesDialogPickMembers([memberLabel]);
    await driver.modulesDialogPickDateRange("2026-11-01", "2026-11-15");
    await driver.modulesDialogSubmitCreate();

    await test.step("the dialog closes with a success notice", async () => {
      // Toast first (it expires), then the close (React unmounts behind it).
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Success!" });
      await expect.poll(() => driver.modulesDialogOpen(), { timeout: 30_000 }).toBe(false);
      await driver.archivesAwaitModuleRow("Full Module");
    });

    await test.step("the server stored every field", async () => {
      const live = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      const created = live.find((row) => row.name === "Full Module");
      expect(created).toBeDefined();
      const detail = await modulesModuleDetail(
        world.workspaceSlug,
        world.projectId,
        created?.id ?? "",
        world.owner.cookie
      );
      expect(detail.description).toBe("Line one\nLine two");
      expect(detail.status).toBe("planned");
      expect(detail.leadId).toBe(world.owner.userId);
      expect(detail.memberIds).toContain(member.userId);
      expect(detail.startDate).toBe("2026-11-01");
      expect(detail.targetDate).toBe("2026-11-15");
    });
  }
);

test(
  specTitle(ROW_023, "bug: NEWFRONT-328 a failed creation reports the error but clears the form"),
  { tag: specTags(ROW_023) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod023b");

    await enterModulesList(driver, world, world.owner);
    await driver.modulesOpenCreateFromHeader();
    await driver.modulesDialogFillTitle("Failed Module");
    await driver.modulesDialogFillDescription("Failed description");
    await driver.modulesFailNextCreate();
    await driver.modulesDialogSubmitCreate();

    // The dialog stays open with an error notice (the green half of
    // MOD-023), but the old form resets to defaults instead of keeping
    // the user's input (NEWFRONT-328): the intended behavior is input
    // intact.
    await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Error!" });
    expect(await driver.modulesDialogOpen()).toBe(true);
    if (driver.target === "web_new") {
      // Fixed (NEWFRONT-328): the input stays intact for correction.
      expect(await driver.modulesDialogTitle()).toBe("Failed Module");
      expect(await driver.modulesDialogDescription()).toBe("Failed description");
    } else {
      expect(await driver.modulesDialogTitle()).toBe("");
      expect(await driver.modulesDialogDescription()).toBe("");
    }
    expect(await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).toEqual([]);
    await driver.modulesDialogCancel();
  }
);

test(
  specTitle(ROW_024, "edit dialog prefills every field and saves"),
  { tag: specTags(ROW_024) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod024");
    const member = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.MEMBER, "parity-mod024m");
    await markOnboardedForProjects(member);
    await setLastWorkspaceForProjects(member, world.workspaceId);
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
      { member_id: member.userId, role: ROLE.MEMBER },
    ]);
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "Prefill Me", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      moduleId,
      {
        description: "Prefill description",
        status: "planned",
        start_date: "2026-10-01",
        target_date: "2026-10-31",
        lead_id: world.owner.userId,
        member_ids: [member.userId],
      },
      world.owner.cookie
    );

    await enterModulesList(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Prefill Me");
    await driver.archivesChooseLiveModuleMenuEntry("Prefill Me", "Edit");

    const leadLabels = await driver.modulesDialogLeadOptions();
    const memberLabel = leadLabels.find((label) => label !== "You") ?? "";
    expect(memberLabel).not.toBe("");

    await test.step("the dialog opens prefilled with no project picker", async () => {
      expect(await driver.modulesDialogHeading()).toBe("Update module");
      expect(await driver.modulesDialogHasProjectPicker()).toBe(false);
      expect(await driver.modulesDialogTitle()).toBe("Prefill Me");
      expect(await driver.modulesDialogDescription()).toBe("Prefill description");
      expect(await driver.modulesDialogStatus()).toContain("Planned");
      expect(await driver.modulesDialogLead()).toBe("You");
      expect(await driver.modulesDialogMembers()).toContain(memberLabel);
      expect(await driver.modulesDialogDateRange()).not.toContain("Start date");
    });

    await test.step("saving stores the edits", async () => {
      await driver.modulesDialogFillTitle("Prefill Edited");
      await driver.modulesDialogPickStatus("In Progress");
      await driver.modulesDialogSubmitUpdate();
      // Toast first (it expires), then the close (React unmounts behind it).
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Success!" });
      await expect.poll(() => driver.modulesDialogOpen(), { timeout: 30_000 }).toBe(false);
      const detail = await modulesModuleDetail(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      expect(detail.name).toBe("Prefill Edited");
      expect(detail.status).toBe("in-progress");
    });
  }
);

test(
  specTitle(ROW_024, "bug: NEWFRONT-328 a failed save reports the error but clears the form"),
  { tag: specTags(ROW_024) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod024b");
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "Save Fail", world.owner.cookie);

    await enterModulesList(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Save Fail");
    await driver.archivesChooseLiveModuleMenuEntry("Save Fail", "Edit");
    await driver.modulesDialogFillTitle("Save Fail Edited");
    await driver.modulesFailNextUpdate();
    await driver.modulesDialogSubmitUpdate();

    // Same reset bug as creation (NEWFRONT-328 covers both modes): the
    // old dialog stays open with an error notice, but the title clears
    // to the empty default instead of keeping the user's edit.
    await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Error!" });
    expect(await driver.modulesDialogOpen()).toBe(true);
    if (driver.target === "web_new") {
      // Fixed (NEWFRONT-328): the edit stays intact for correction.
      expect(await driver.modulesDialogTitle()).toBe("Save Fail Edited");
    } else {
      expect(await driver.modulesDialogTitle()).toBe("");
    }
    const detail = await modulesModuleDetail(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
    expect(detail.name).toBe("Save Fail");
    await driver.modulesDialogCancel();
  }
);

test(
  specTitle(ROW_025, "delete confirmation names the module and deletes from the list"),
  { tag: specTags(ROW_025) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod025");
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "Delete Me", world.owner.cookie);

    await enterModulesList(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Delete Me");

    await test.step("the confirmation names the module and warns of permanence", async () => {
      await driver.archivesChooseLiveModuleMenuEntry("Delete Me", "Delete");
      const dialog = await driver.modulesDeleteDialog();
      expect(dialog?.heading).toBe("Delete module");
      expect(dialog?.body).toContain("Delete Me");
      expect(dialog?.body).toContain("permanently removed");
      expect(dialog?.body).toContain("cannot be undone");
    });

    await test.step("cancelling keeps the module", async () => {
      await driver.modulesDeleteCancel();
      expect(await driver.modulesDeleteDialog()).toBeNull();
      expect(await modulesModuleGone(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie)).toBe(false);
    });

    await test.step("confirming deletes without navigating away", async () => {
      await driver.archivesChooseLiveModuleMenuEntry("Delete Me", "Delete");
      await driver.modulesDeleteConfirm();
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Success!" });
      // No navigation: the address keeps its entered form (a push would
      // normalize the trailing slash, as the detail case below shows).
      expect(await driver.currentUrlPath()).toBe(`/${world.workspaceSlug}/projects/${world.projectId}/modules`);
      expect(await modulesModuleGone(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie)).toBe(true);
      await expect.poll(() => driver.archivesModuleRowNames()).not.toContain("Delete Me");
    });
  }
);

test(
  specTitle(ROW_025, "deleting from the detail page returns to the modules list"),
  { tag: specTags(ROW_025) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod025d");
    const moduleId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Detail Delete",
      world.owner.cookie
    );

    await driver.openAuthenticated(
      `/${world.workspaceSlug}/projects/${world.projectId}/modules/${moduleId}`,
      browserSessionCookies(world.owner)
    );
    await driver.modulesOpenDetailMenu();
    await driver.modulesChooseDetailMenuEntry("Delete");
    const dialog = await driver.modulesDeleteDialog();
    expect(dialog?.body).toContain("Detail Delete");

    await driver.modulesDeleteConfirm();
    await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Success!" });
    await expect
      .poll(() => driver.currentUrlPath(), { timeout: 30_000 })
      .toBe(`/${world.workspaceSlug}/projects/${world.projectId}/modules/`);
    expect(await modulesModuleGone(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie)).toBe(true);
  }
);

test(
  specTitle(ROW_025, "bug: NEWFRONT-329 a peek-open delete keeps the peek address"),
  { tag: specTags(ROW_025) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod025p");
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "Peek Delete", world.owner.cookie);

    await enterModulesList(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Peek Delete");
    await driver.archivesOpenLiveModulePeek("Peek Delete");
    expect(await driver.currentPath()).toContain("peekModule=");

    // The row requires navigating back to the modules list; instead the
    // old app stays on the peek address (NEWFRONT-329) while the module
    // is gone server-side.
    await driver.archivesChooseLiveModuleMenuEntry("Peek Delete", "Delete");
    await driver.modulesDeleteConfirm();
    await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Success!" });
    if (driver.target === "web_new") {
      // Fixed (NEWFRONT-329): deleting the peeked module removes the
      // key, closes the panel and lands on the plain list.
      expect(await driver.currentPath()).not.toContain("peekModule=");
      await expect.poll(() => driver.modulesPeekVisible(), { timeout: 30_000 }).toBe(false);
    } else {
      expect(await driver.currentPath()).toContain(`peekModule=${moduleId}`);
    }
    expect(await modulesModuleGone(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie)).toBe(true);
  }
);

test(
  specTitle(ROW_025, "a permission failure shows the dedicated notice and keeps the module"),
  { tag: specTags(ROW_025) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod025f");
    const moduleId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Permission Delete",
      world.owner.cookie
    );

    await enterModulesList(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Permission Delete");
    await driver.archivesChooseLiveModuleMenuEntry("Permission Delete", "Delete");
    expect(await driver.modulesDeleteDialog()).not.toBeNull();

    // The shape the server returns for forbidden deletes (verified in
    // the permission class): {"error": "You don't have the required
    // permissions."} with HTTP 403.
    await driver.modulesFailDeletePermission();
    await driver.modulesDeleteConfirm();
    await expect
      .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
      .toMatchObject({
        title: "You don't have permission to perform this action.",
      });
    expect(await driver.modulesDeleteDialog()).toBeNull();
    expect(await modulesModuleGone(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie)).toBe(false);
    expect(await driver.archivesModuleRowNames()).toContain("Permission Delete");
  }
);

test(
  specTitle(ROW_026, "archive confirmation archives a finished module and returns to the list"),
  { tag: specTags(ROW_026) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod026");
    const confirmed = await createModuleWithStatus(world, "Archive Yes", "completed");
    const cancelled = await createModuleWithStatus(world, "Archive No", "cancelled");

    await enterModulesList(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Archive Yes");

    await test.step("confirming archives with a notice pointing at the project archives", async () => {
      await driver.archivesChooseLiveModuleMenuEntry("Archive Yes", "Archive");
      const dialog = await driver.archivesArchiveDialog();
      expect(dialog?.title).toContain("Archive Yes");
      expect(dialog?.body).toContain("restored");
      await driver.archivesConfirmArchiveDialog();
      // The new notice words the landing differently ("this project's
      // archives"); the old copy says "project archives". The row pins
      // the notice pointing at the project archives, not the prose.
      const archiveLandingMention = driver.target === "web_new" ? "archives" : "project archives";
      await expect
        .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
        .toMatchObject({
          title: "Archive success",
          message: expect.stringContaining(archiveLandingMention),
        });
      // The new landing keeps the bare path (no client-side hop runs);
      // the old hop adds its trailing slash (slash-tolerant row).
      const archiveLandingPath = (await driver.currentUrlPath()).replace(/\/$/, "");
      expect(archiveLandingPath).toBe(`/${world.workspaceSlug}/projects/${world.projectId}/modules`);
      expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, confirmed, world.owner.cookie)).toBe(
        true
      );
      await expect.poll(() => driver.archivesModuleRowNames()).not.toContain("Archive Yes");
    });

    await test.step("cancelling changes nothing", async () => {
      await driver.archivesChooseLiveModuleMenuEntry("Archive No", "Archive");
      expect(await driver.archivesArchiveDialog()).not.toBeNull();
      await driver.archivesCancelArchiveDialog();
      expect(await driver.archivesArchiveDialog()).toBeNull();
      expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, cancelled, world.owner.cookie)).toBe(
        false
      );
    });
  }
);

test(
  specTitle(ROW_026, "bug: NEWFRONT-233 a failed archive reports success and navigates anyway"),
  { tag: specTags(ROW_026) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod026b");
    const failing = await createModuleWithStatus(world, "Archive Fail", "completed");

    await enterModulesList(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Archive Fail");

    // The old store swallows the write error (NEWFRONT-233, filed by
    // NEWFRONT-225), so the dialog runs its success path: success
    // notice, close, navigate — the module stays live.
    await driver.archivesFailNextModuleWrite();
    await driver.archivesChooseLiveModuleMenuEntry("Archive Fail", "Archive");
    await driver.archivesConfirmArchiveDialog();
    if (driver.target === "web_new") {
      // Fixed (NEWFRONT-233): the failed write reports its error, the
      // dialog closes so the viewer can retry, and nothing navigates
      // because the module is still live (slash-tolerant: no hop runs,
      // so the address keeps its direct-entry shape).
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Error!" });
      // Toasts carry role=dialog (cycles-driver precedent), so the
      // error notice must drain before the dialog read proves the
      // archive confirmation closed.
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toBe(null);
      expect(await driver.archivesArchiveDialog()).toBeNull();
      expect((await driver.currentUrlPath()).replace(/\/$/, "")).toBe(
        `/${world.workspaceSlug}/projects/${world.projectId}/modules`
      );
    } else {
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Archive success" });
      expect(await driver.archivesArchiveDialog()).toBeNull();
      expect(await driver.currentUrlPath()).toBe(`/${world.workspaceSlug}/projects/${world.projectId}/modules/`);
    }
    expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, failing, world.owner.cookie)).toBe(
      false
    );
    expect(await driver.archivesModuleRowNames()).toContain("Archive Fail");
  }
);

test(
  specTitle(ROW_027, "archived screen lives under the archives strip with module filters and reusable rows"),
  { tag: specTags(ROW_027) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod027");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Live Twin", world.owner.cookie);
    const leaded = await serverCreateModule(world.workspaceSlug, world.projectId, "Archived Owned", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      leaded,
      { status: "completed", lead_id: world.owner.userId },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, leaded, world.owner.cookie);
    const open = await serverCreateModule(world.workspaceSlug, world.projectId, "Archived Open", world.owner.cookie);
    await archivesPatchModule(world.workspaceSlug, world.projectId, open, { status: "cancelled" }, world.owner.cookie);
    await archivesArchiveModule(world.workspaceSlug, world.projectId, open, world.owner.cookie);

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Archived Owned");

    await test.step("the screen sits on the archives tab strip", async () => {
      expect(await driver.archivesTabNames()).toContain("Modules");
      expect(await driver.archivesActiveTab()).toBe("Modules");
      // Trailing-slash tolerant: the old client's hops add one, the new
      // app serves the bare path on direct entry; the row pins the
      // address, not the slash.
      expect((await driver.currentUrlPath()).replace(/\/$/, "")).toBe(
        `/${world.workspaceSlug}/projects/${world.projectId}/archives/modules`
      );
    });

    await test.step("filters cover lead, members and dates without favorites or status", async () => {
      await driver.archivesOpenModuleFilters();
      const groups = await driver.archivesModuleFilterGroups();
      expect(groups).toEqual(["Lead", "Members", "Start date", "Due date"]);
      expect(groups).not.toContain("Favorites");
      expect(groups).not.toContain("Status");
      await driver.archivesCloseModuleFilters();
    });

    await test.step("applied chips remove individually and clear all at once", async () => {
      await driver.archivesOpenModuleFilters();
      await driver.archivesToggleLeadFilter("You");
      await driver.archivesCloseModuleFilters();
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Archived Owned"]);
      expect((await driver.archivesModuleChips()).map((chip) => chip.key)).toEqual(["lead"]);
      await driver.archivesRemoveModuleChip("lead");
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Archived Open", "Archived Owned"]);
      await driver.archivesOpenModuleFilters();
      await driver.archivesToggleLeadFilter("You");
      await driver.archivesCloseModuleFilters();
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Archived Owned"]);
      await driver.archivesClearModuleFilters();
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Archived Open", "Archived Owned"]);
      expect(await driver.archivesModuleChips()).toEqual([]);
    });

    await test.step("archived rows reuse the standard list row", async () => {
      const archived = await driver.modulesRowFacts("Archived Owned");
      expect(archived.nameHref).toMatch(/\/modules\/[0-9a-fA-F-]{36}\/?(?:\?.*)?$/);
      expect(archived.hasPeek).toBe(true);
      await driver.archivesOpenLiveModules(world.workspaceSlug, world.projectId);
      await driver.archivesAwaitModuleRow("Live Twin");
      const live = await driver.modulesRowFacts("Live Twin");
      expect(live.nameHref).toMatch(/\/modules\/[0-9a-fA-F-]{36}\/?(?:\?.*)?$/);
      expect(live.hasPeek).toBe(true);
    });

    await test.step("an archived row opens the archived peek panel", async () => {
      await driver.archivesOpenModulesTab(world.workspaceSlug, world.projectId);
      await driver.archivesAwaitModuleRow("Archived Owned");
      await driver.archivesOpenModulePeek("Archived Owned");
      expect(await driver.archivesModulePeekName()).toBe("Archived Owned");
      expect(await driver.currentPath()).toContain("peekModule=");
    });
  }
);

test(
  specTitle(ROW_027, "archived empties guide the zero-archive and no-match states"),
  { tag: specTags(ROW_027) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod027e");

    await enterArchivesModules(driver, world, world.owner);

    await test.step("zero archived modules shows the onboarding empty", async () => {
      expect(await driver.archivesModulesEmptyKind()).toBe("zero");
      expect(await archivesArchivedModules(world.workspaceSlug, world.projectId, world.owner.cookie)).toEqual([]);
    });

    const archived = await serverCreateModule(world.workspaceSlug, world.projectId, "Empty Alpha", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      archived,
      { status: "completed" },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, archived, world.owner.cookie);
    await driver.archivesOpenModulesTab(world.workspaceSlug, world.projectId);
    await driver.archivesAwaitModuleRow("Empty Alpha");

    await test.step("search hiding everything shows the search guidance", async () => {
      await driver.archivesOpenModuleSearch();
      await driver.archivesTypeModuleSearch("no-such-module-zzz");
      await expect.poll(() => driver.archivesModulesEmptyKind()).toBe("search");
      await driver.archivesClearModuleSearch();
      await expect.poll(() => driver.archivesModulesEmptyKind()).toBe("rows");
    });

    await test.step("filters hiding everything shows the filters guidance", async () => {
      await driver.archivesOpenModuleFilters();
      await driver.archivesToggleLeadFilter("You");
      await driver.archivesCloseModuleFilters();
      await expect.poll(() => driver.archivesModulesEmptyKind()).toBe("filters");
      await driver.archivesClearModuleFilters();
      await expect.poll(() => driver.archivesModulesEmptyKind()).toBe("rows");
    });
  }
);

test(
  specTitle(ROW_028, "restore returns the module to the active list"),
  { tag: specTags(ROW_028) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod028");
    const restored = await serverCreateModule(world.workspaceSlug, world.projectId, "Restore Me", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      restored,
      { status: "completed" },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, restored, world.owner.cookie);

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Restore Me");

    await driver.archivesChooseArchivedModuleMenuEntry("Restore Me", "Restore");
    await expect
      .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
      .toMatchObject({
        title: "Restore success",
        message: expect.stringContaining("project modules"),
      });
    // The row's "archives overview" is the archives modules screen the
    // restore lands on (slash-tolerant: see the tab-strip step above).
    expect((await driver.currentUrlPath()).replace(/\/$/, "")).toBe(
      `/${world.workspaceSlug}/projects/${world.projectId}/archives/modules`
    );
    expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, restored, world.owner.cookie)).toBe(
      false
    );
    await expect.poll(() => driver.archivesModuleRowNames()).not.toContain("Restore Me");
    const live = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
    expect(live.map((row) => row.name)).toContain("Restore Me");
  }
);

test(
  specTitle(ROW_028, "bug: NEWFRONT-233 a failed restore reports success and keeps the row"),
  { tag: specTags(ROW_028) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod028b");
    const failing = await serverCreateModule(world.workspaceSlug, world.projectId, "Restore Fail", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      failing,
      { status: "cancelled" },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, failing, world.owner.cookie);

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Restore Fail");

    // Same swallowed write error as the archive path (NEWFRONT-233):
    // success notice, same-tab stay — the module stays archived.
    await driver.archivesFailNextModuleWrite();
    await driver.archivesChooseArchivedModuleMenuEntry("Restore Fail", "Restore");
    if (driver.target === "web_new") {
      // Fixed (NEWFRONT-233): the failed write reports its error and
      // the row stays; the tab never leaves the archived screen.
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Error!" });
    } else {
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Restore success" });
    }
    expect((await driver.currentUrlPath()).replace(/\/$/, "")).toBe(
      `/${world.workspaceSlug}/projects/${world.projectId}/archives/modules`
    );
    expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, failing, world.owner.cookie)).toBe(
      true
    );
    expect(await driver.archivesModuleRowNames()).toContain("Restore Fail");
  }
);
