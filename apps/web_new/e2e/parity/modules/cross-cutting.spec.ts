// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-311): modules settings toggle, command-palette
// creation and pickers, shareable deep links, and the no-live-updates,
// keyboard, no-drag/export, desktop and edition negatives. Rows MOD-036
// through MOD-043. Green on apps/web first.
//
// Fixture shape: every scenario mints its own owner, workspace, and project
// (the modules flag and display choices persist per project), because the
// seed carries no modules and several scenarios flip the flag. Modules,
// issues, and memberships are created through the API in-spec.
//
// Palette command titles below ("New module", "Open a module",
// "Add to modules") are the old app's user-visible labels, read the same
// way every oracle spec reads them; the work-item picker only exists on
// /browse/ routes, where the palette gains its work-item context.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  ROLE,
  addProjectMembersViaApi,
  archivesArchiveModule,
  archivesArchivedModules,
  archivesPatchModule,
  browserSessionCookies,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  modulesXIssueModuleIds,
  modulesXModuleDetail,
  modulesXSetModuleView,
  projectMemberRole,
  seatFreshMember,
  serverAddIssuesToModule,
  serverCreateIssue,
  serverCreateModule,
  serverCreateProjectWithFlags,
  serverIssueDetail,
  serverProjectDetails,
  serverProjectModules,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { entryTrafficViolations } from "./entryTraffic";

const ROW_036 = ["MOD-036"];
const ROW_037 = ["MOD-037"];
const ROW_038 = ["MOD-038"];
const ROW_039 = ["MOD-039"];
const ROW_040 = ["MOD-040"];
const ROW_041 = ["MOD-041"];
const ROW_042 = ["MOD-042"];
const ROW_043 = ["MOD-043"];

/** Settle window after a behind-the-scenes mutation (NTF-028 precedent). */
const SETTLE_MS = 3_000;

interface CrosscuttingWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  projectId: string;
  projectName: string;
}

/** Fresh owner, workspace, and project for one scenario (retried create). */
async function setupCrosscuttingWorld(prefix: string, moduleView: boolean): Promise<CrosscuttingWorld> {
  const owner = await signUpAuthedSession(prefix);
  // The project create 400s intermittently on a loaded scratch stack; one
  // retry with a fresh suffix rides it out (archives precedent).
  for (let attempt = 1; ; attempt++) {
    const suffix = uniqueSuffixForProjects();
    try {
      const ws = await createWorkspaceForProjects(owner, {
        name: `ModX WS ${suffix}`,
        slug: `modx-${suffix}`,
      });
      await markOnboardedForProjects(owner);
      await setLastWorkspaceForProjects(owner, ws.id);
      const projectName = `Crosscutting ${suffix}`;
      const projectId = await serverCreateProjectWithFlags(
        ws.slug,
        projectName,
        `MX${suffix.slice(-8)}`.toUpperCase(),
        { moduleView },
        owner.cookie
      );
      return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, projectId, projectName };
    } catch (error) {
      if (attempt >= 2) throw error;
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
  }
}

/** Seat a user in the workspace and optionally the project, onboarded for UI. */
async function seatCrosscuttingMember(
  world: CrosscuttingWorld,
  workspaceRole: number,
  projectRole: number | null,
  prefix: string
): Promise<AuthedSession> {
  const member = await seatFreshMember(world.owner, world.workspaceSlug, workspaceRole, prefix);
  await markOnboardedForProjects(member);
  await setLastWorkspaceForProjects(member, world.workspaceId);
  if (projectRole !== null) {
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
      { member_id: member.userId, role: projectRole },
    ]);
  }
  return member;
}

/** Enter the modules list as `session`; resolves once the named row shows. */
async function enterModulesListWith(
  driver: ParityDriver,
  world: CrosscuttingWorld,
  session: AuthedSession,
  name: string
): Promise<void> {
  await driver.openAuthenticated(
    `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
    browserSessionCookies(session)
  );
  await expect.poll(async () => driver.modulesXListNames(), { timeout: 60_000 }).toContain(name);
}

/** The modules flag as the server reports it (polled: the UI PATCH races). */
async function awaitModuleViewFlag(world: CrosscuttingWorld, session: AuthedSession, want: boolean): Promise<void> {
  await expect
    .poll(async () => (await serverProjectDetails(world.workspaceSlug, world.projectId, session.cookie)).moduleView, {
      timeout: 30_000,
    })
    .toBe(want);
}

test(
  specTitle(ROW_036, "settings toggle gates the modules list both ways; members see a notice"),
  { tag: specTags(ROW_036) },
  async ({ driver }) => {
    const world = await setupCrosscuttingWorld("modx-036", true);
    const moduleName = `Toggle module ${uniqueSuffixForProjects()}`;
    await serverCreateModule(world.workspaceSlug, world.projectId, moduleName, world.owner.cookie);

    await test.step("list shows the module while the flag is on", async () => {
      await enterModulesListWith(driver, world, world.owner, moduleName);
      expect(await driver.modulesXListGateVisible()).toBe(false);
      await awaitModuleViewFlag(world, world.owner, true);
    });

    await test.step("flipping the toggle off gates the list", async () => {
      await driver.modulesXOpenSettingsModules(world.workspaceSlug, world.projectId);
      expect(await driver.modulesXSettingsShowsNotAuthorized()).toBe(false);
      expect(await driver.modulesXModulesToggleState()).toBe(true);
      await driver.modulesXFlipModulesToggle();
      expect(await driver.modulesXModulesToggleState()).toBe(false);
      await awaitModuleViewFlag(world, world.owner, false);
      await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
      expect(await driver.modulesXListGateVisible()).toBe(true);
      expect(await driver.modulesXListNames()).toEqual([]);
      const gate = await driver.modulesXGateManageFeaturesState();
      expect(gate.visible).toBe(true);
      expect(gate.disabled).toBe(false);
    });

    await test.step("flipping the toggle on brings the list back", async () => {
      await driver.modulesXOpenSettingsModules(world.workspaceSlug, world.projectId);
      await driver.modulesXFlipModulesToggle();
      expect(await driver.modulesXModulesToggleState()).toBe(true);
      await awaitModuleViewFlag(world, world.owner, true);
      await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
      expect(await driver.modulesXListGateVisible()).toBe(false);
      await expect.poll(async () => driver.modulesXListNames(), { timeout: 30_000 }).toContain(moduleName);
    });

    await test.step("a project member sees a notice instead of the toggle", async () => {
      const member = await seatCrosscuttingMember(world, ROLE.MEMBER, ROLE.MEMBER, "modx-036-member");
      expect(await projectMemberRole(member, world.workspaceSlug, world.projectId)).toBe(ROLE.MEMBER);
      await driver.openAuthenticated(
        `/${world.workspaceSlug}/settings/projects/${world.projectId}/features/modules`,
        browserSessionCookies(member)
      );
      // The settings entry settles on either branch; poll for a decision.
      await expect
        .poll(
          async () =>
            (await driver.modulesXSettingsShowsNotAuthorized())
              ? "denied"
              : (await driver.modulesXModulesToggleState()) === null
                ? "loading"
                : "toggle",
          { timeout: 60_000 }
        )
        .toBe("denied");
      expect(await driver.modulesXModulesToggleState()).toBe(null);
    });
  }
);

test(
  specTitle(ROW_037, "palette create command is gated by context, flag, and role, and opens the create dialog"),
  { tag: specTags(ROW_037) },
  async ({ driver }) => {
    const world = await setupCrosscuttingWorld("modx-037a", true);
    const moduleName = `Palette module ${uniqueSuffixForProjects()}`;
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, moduleName, world.owner.cookie);

    await test.step("member inside an enabled project sees the create command", async () => {
      const member = await seatCrosscuttingMember(world, ROLE.MEMBER, ROLE.MEMBER, "modx-037a-member");
      await enterModulesListWith(driver, world, member, moduleName);
      await driver.pressPaletteOpenChord();
      expect(await driver.isCommandPaletteOpen()).toBe(true);
      expect(await driver.paletteHasCommand("New module")).toBe(true);
      await driver.modulesXPressKey("Escape");
    });

    await test.step("activating it opens the create dialog and writes nothing on dismiss", async () => {
      await enterModulesListWith(driver, world, world.owner, moduleName);
      await driver.pressPaletteOpenChord();
      await driver.activatePaletteCommand("New module");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(true);
      await driver.modulesXPressKey("Escape");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(false);
      const rows = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      expect(rows.map((row) => row.id).sort()).toEqual([moduleId]);
    });

    await test.step("no project context hides the command", async () => {
      await driver.openAuthenticated(`/${world.workspaceSlug}/settings`, browserSessionCookies(world.owner));
      await driver.pressPaletteOpenChord();
      expect(await driver.isCommandPaletteOpen()).toBe(true);
      expect(await driver.paletteHasCommand("New module")).toBe(false);
      await driver.modulesXPressKey("Escape");
    });

    await test.step("a flag-off project hides the command", async () => {
      await modulesXSetModuleView(world.workspaceSlug, world.projectId, false, world.owner.cookie);
      await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
      expect(await driver.modulesXListGateVisible()).toBe(true);
      await driver.pressPaletteOpenChord();
      expect(await driver.paletteHasCommand("New module")).toBe(false);
      await driver.modulesXPressKey("Escape");
      await modulesXSetModuleView(world.workspaceSlug, world.projectId, true, world.owner.cookie);
      await awaitModuleViewFlag(world, world.owner, true);
    });

    await test.step("a project guest sees the list but no create command", async () => {
      const guest = await seatCrosscuttingMember(world, ROLE.MEMBER, ROLE.GUEST, "modx-037a-guest");
      expect(await projectMemberRole(guest, world.workspaceSlug, world.projectId)).toBe(ROLE.GUEST);
      await enterModulesListWith(driver, world, guest, moduleName);
      await driver.pressPaletteOpenChord();
      expect(await driver.paletteHasCommand("New module")).toBe(false);
      await driver.modulesXPressKey("Escape");
    });
  }
);

test(
  specTitle(ROW_037, "palette jump menu lists modules with status icons; work-item picker preselects membership"),
  { tag: specTags(ROW_037) },
  async ({ driver }) => {
    const world = await setupCrosscuttingWorld("modx-037b", true);
    const suffix = uniqueSuffixForProjects();
    const firstName = `Jump first ${suffix}`;
    const secondName = `Jump second ${suffix}`;
    const firstId = await serverCreateModule(world.workspaceSlug, world.projectId, firstName, world.owner.cookie);
    const secondId = await serverCreateModule(world.workspaceSlug, world.projectId, secondName, world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      firstId,
      { status: "in-progress" },
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      secondId,
      { status: "planned" },
      world.owner.cookie
    );
    const issueName = `Picker work item ${suffix}`;
    const issueId = await serverCreateIssue(world.workspaceSlug, world.projectId, world.owner.cookie, issueName);
    await serverAddIssuesToModule(world.workspaceSlug, world.projectId, firstId, [issueId], world.owner.cookie);

    await test.step("jump menu lists the project's modules with status icons", async () => {
      await enterModulesListWith(driver, world, world.owner, firstName);
      await driver.pressPaletteOpenChord();
      await expect.poll(async () => driver.paletteHasCommand("Open a module"), { timeout: 60_000 }).toBe(true);
      await driver.activatePaletteCommand("Open a module");
      await expect
        .poll(async () => (await driver.modulesXPaletteModuleMenuItems()).length, { timeout: 60_000 })
        .toBeGreaterThan(0);
      const items = await driver.modulesXPaletteModuleMenuItems();
      const serverRows = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      expect(items.map((item) => item.name).sort()).toEqual(serverRows.map((row) => row.name).sort());
      expect(items.length).toBe(2);
      for (const item of items) {
        expect(item.hasIcon).toBe(true);
        expect(item.hasCheck).toBe(false);
      }
      const detail = await modulesXModuleDetail(world.workspaceSlug, world.projectId, firstId, world.owner.cookie);
      expect(detail.status).toBe("in-progress");
    });

    await test.step("picking a jump entry navigates to its detail", async () => {
      await driver.modulesXActivatePaletteModuleMenuItem(firstName);
      await expect.poll(async () => driver.currentPath(), { timeout: 30_000 }).toContain(firstId);
      await expect.poll(async () => driver.modulesXDetailName(), { timeout: 30_000 }).toBe(firstName);
    });

    await test.step("work-item picker lists modules with membership preselected", async () => {
      const identifiers = await serverIssueDetail(world.workspaceSlug, world.projectId, issueId, world.owner.cookie);
      // The merged issues entry waits for the work item to load; opening
      // the palette before that leaves the contextual commands missing.
      // (Owner cookies persist from this test's earlier steps.)
      await driver.composerOpenIssue(
        world.workspaceSlug,
        `${identifiers.project_identifier}-${identifiers.sequence_id}`
      );
      await driver.pressPaletteOpenChord();
      await expect.poll(async () => driver.paletteHasCommand("Add to modules"), { timeout: 60_000 }).toBe(true);
      await driver.activatePaletteCommand("Add to modules");
      await expect
        .poll(async () => (await driver.modulesXPaletteModuleMenuItems()).length, { timeout: 60_000 })
        .toBeGreaterThan(0);
      // The membership check renders once the work-item details land,
      // possibly after the rows themselves.
      await expect
        .poll(
          async () =>
            (await driver.modulesXPaletteModuleMenuItems()).find((item) => item.name === firstName)?.hasCheck ?? null,
          { timeout: 60_000 }
        )
        .toBe(true);
      const items = await driver.modulesXPaletteModuleMenuItems();
      expect(items.map((item) => item.name).sort()).toEqual([firstName, secondName].sort());
      const first = items.find((item) => item.name === firstName);
      const second = items.find((item) => item.name === secondName);
      expect(first?.hasCheck).toBe(true);
      expect(second?.hasCheck).toBe(false);
      expect(await modulesXIssueModuleIds(world.workspaceSlug, world.projectId, issueId, world.owner.cookie)).toEqual([
        firstId,
      ]);
      // Picker writes belong to the issues area: leave without selecting.
      await driver.modulesXPressKey("Escape");
      await driver.modulesXPressKey("Escape");
      expect(await modulesXIssueModuleIds(world.workspaceSlug, world.projectId, issueId, world.owner.cookie)).toEqual([
        firstId,
      ]);
    });
  }
);

test(
  specTitle(ROW_038, "copied links reopen detail and peek; unknown ids and outsiders are fenced"),
  { tag: specTags(ROW_038) },
  async ({ driver }) => {
    const world = await setupCrosscuttingWorld("modx-038", true);
    const moduleName = `Linked module ${uniqueSuffixForProjects()}`;
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, moduleName, world.owner.cookie);

    await test.step("a copied detail link loads the same module", async () => {
      await driver.openAuthenticated(
        `/${world.workspaceSlug}/projects/${world.projectId}/modules/${moduleId}`,
        browserSessionCookies(world.owner)
      );
      await expect.poll(async () => driver.modulesXDetailName(), { timeout: 60_000 }).toBe(moduleName);
      expect(await driver.modulesXDetailMissingVisible()).toBe(false);
      const detail = await modulesXModuleDetail(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      expect(detail.name).toBe(moduleName);
    });

    await test.step("a copied peek link reopens the same panel", async () => {
      await driver.modulesXOpenModulesListWithPeek(world.workspaceSlug, world.projectId, moduleId);
      expect(await driver.modulesXPeekName()).toBe(moduleName);
      expect(await driver.modulesXListNames()).toContain(moduleName);
    });

    await test.step("an unresolvable module id shows the missing-module state", async () => {
      const bogusId = "00000000-0000-0000-0000-000000000000";
      await driver.modulesXOpenModuleDetail(world.workspaceSlug, world.projectId, bogusId);
      expect(await driver.modulesXDetailMissingVisible()).toBe(true);
      expect(await driver.modulesXDetailName()).toBe(null);
      await driver.modulesXMissingModuleBackToList();
      await expect.poll(async () => driver.modulesXListNames(), { timeout: 30_000 }).toContain(moduleName);
    });

    await test.step("a user without project access gets the project wall, not content", async () => {
      // Oracle correction to the row: non-members meet the shared project
      // access wall (with its join action), not the missing-module state;
      // that state is for unresolvable ids, proven above.
      const outsider = await seatCrosscuttingMember(world, ROLE.MEMBER, null, "modx-038-outsider");
      expect(await projectMemberRole(outsider, world.workspaceSlug, world.projectId)).toBe(null);
      await driver.openAuthenticated(
        `/${world.workspaceSlug}/projects/${world.projectId}/modules/${moduleId}`,
        browserSessionCookies(outsider)
      );
      await expect.poll(async () => driver.modulesXProjectAccessWallVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.modulesXDetailName()).toBe(null);
      expect(await driver.modulesXDetailMissingVisible()).toBe(false);
      // The module still exists: the wall is access-driven.
      const detail = await modulesXModuleDetail(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      expect(detail.name).toBe(moduleName);
    });
  }
);

test(
  specTitle(ROW_039, "list and peek stay stale until navigation; own writes show at once"),
  { tag: specTags(ROW_039) },
  async ({ driver }) => {
    const world = await setupCrosscuttingWorld("modx-039", true);
    const suffix = uniqueSuffixForProjects();
    const firstName = `Live first ${suffix}`;
    const behindName = `Live behind ${suffix}`;
    const ownName = `Live own ${suffix}`;
    const firstId = await serverCreateModule(world.workspaceSlug, world.projectId, firstName, world.owner.cookie);
    const member = await seatCrosscuttingMember(world, ROLE.MEMBER, ROLE.MEMBER, "modx-039-member");

    await test.step("a module created behind the list stays invisible until reload", async () => {
      await enterModulesListWith(driver, world, world.owner, firstName);
      // Drain late mount fetches before mutating behind the page, so a
      // fetch that started at load can never land after the mutation.
      await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
      expect(await driver.modulesXListNames()).toEqual([firstName]);
      await serverCreateModule(world.workspaceSlug, world.projectId, behindName, member.cookie);
      await expect
        .poll(async () => serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie), {
          timeout: 30_000,
        })
        .toEqual(expect.arrayContaining([expect.objectContaining({ name: behindName })]));
      await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
      expect(await driver.modulesXListNames()).toEqual([firstName]);
      await driver.reloadPage();
      await expect
        .poll(async () => driver.modulesXListNames(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([firstName, behindName]));
    });

    await test.step("a rename behind the peek stays invisible until reload", async () => {
      const repeeked = `${firstName} repeeked`;
      await driver.modulesXOpenModulesListWithPeek(world.workspaceSlug, world.projectId, firstId);
      const before = await driver.modulesXPeekName();
      expect(before).toBe(firstName);
      await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
      expect(await driver.modulesXPeekName()).toBe(firstName);
      await archivesPatchModule(world.workspaceSlug, world.projectId, firstId, { name: repeeked }, member.cookie);
      await expect
        .poll(async () => modulesXModuleDetail(world.workspaceSlug, world.projectId, firstId, world.owner.cookie), {
          timeout: 30_000,
        })
        .toEqual(expect.objectContaining({ name: repeeked }));
      await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
      expect(await driver.modulesXPeekName()).toBe(before);
      await driver.reloadPage();
      await expect.poll(async () => driver.modulesXPeekName(), { timeout: 60_000 }).toBe(repeeked);
    });

    await test.step("the acting user's own creation shows without a reload", async () => {
      await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
      await driver.pressPaletteOpenChord();
      await driver.activatePaletteCommand("New module");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(true);
      await driver.modulesXFillCreateName(ownName);
      await driver.modulesXSubmitCreate();
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 30_000 }).toBe(false);
      await expect.poll(async () => driver.modulesXListNames(), { timeout: 60_000 }).toContain(ownName);
      const rows = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      expect(rows.map((row) => row.name)).toContain(ownName);
    });
  }
);

test(
  specTitle(
    ROW_039,
    "bug: NEWFRONT-332 detail picks up other users' renames without navigation (intended: stays stale)"
  ),
  { tag: specTags(ROW_039) },
  async ({ driver }) => {
    const world = await setupCrosscuttingWorld("modx-039bug", true);
    const firstName = `Live bug ${uniqueSuffixForProjects()}`;
    const renamed = `${firstName} renamed`;
    const firstId = await serverCreateModule(world.workspaceSlug, world.projectId, firstName, world.owner.cookie);
    const member = await seatCrosscuttingMember(world, ROLE.MEMBER, ROLE.MEMBER, "modx-039bug-member");

    await driver.openAuthenticated(
      `/${world.workspaceSlug}/projects/${world.projectId}/modules/${firstId}`,
      browserSessionCookies(world.owner)
    );
    await expect.poll(async () => driver.modulesXDetailName(), { timeout: 60_000 }).toBe(firstName);
    await archivesPatchModule(world.workspaceSlug, world.projectId, firstId, { name: renamed }, member.cookie);
    await expect
      .poll(async () => modulesXModuleDetail(world.workspaceSlug, world.projectId, firstId, world.owner.cookie), {
        timeout: 30_000,
      })
      .toEqual(expect.objectContaining({ name: renamed }));
    // Actual: the detail revalidates behind the scenes (SWR defaults) and
    // shows the rename with no navigation and no reload. Intended (row):
    // it stays stale like the list and peek.
    await expect.poll(async () => driver.modulesXDetailName(), { timeout: 60_000 }).toBe(renamed);
  }
);

test(
  specTitle(ROW_040, "Escape closes dialogs and search; tab order is logical; only the palette shortcut fires"),
  { tag: specTags(ROW_040) },
  async ({ driver }) => {
    const world = await setupCrosscuttingWorld("modx-040", true);
    const moduleName = `Keys module ${uniqueSuffixForProjects()}`;
    await serverCreateModule(world.workspaceSlug, world.projectId, moduleName, world.owner.cookie);

    await test.step("Escape closes the create dialog", async () => {
      await enterModulesListWith(driver, world, world.owner, moduleName);
      await driver.pressPaletteOpenChord();
      await driver.activatePaletteCommand("New module");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(true);
      await driver.modulesXPressKey("Escape");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(false);
    });

    await test.step("Escape closes the edit dialog", async () => {
      await driver.modulesXOpenEditDialogFromRow(moduleName);
      expect(await driver.modulesXModuleDialogOpen()).toBe(true);
      await driver.modulesXPressKey("Escape");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(false);
      const rows = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      expect(rows.map((row) => row.name)).toEqual([moduleName]);
    });

    await test.step("Escape clears the list search, then collapses it", async () => {
      await driver.modulesXOpenListSearch();
      await driver.modulesXFillListSearch("zzz-no-match");
      expect(await driver.modulesXListSearchState()).toEqual({ expanded: true, query: "zzz-no-match" });
      await driver.modulesXPressKey("Escape");
      await expect
        .poll(async () => driver.modulesXListSearchState(), { timeout: 15_000 })
        .toEqual({
          expanded: true,
          query: "",
        });
      await driver.modulesXPressKey("Escape");
      await expect
        .poll(async () => driver.modulesXListSearchState(), { timeout: 15_000 })
        .toEqual({
          expanded: false,
          query: "",
        });
      await expect.poll(async () => driver.modulesXListNames(), { timeout: 30_000 }).toContain(moduleName);
    });

    await test.step("the create dialog exposes a logical tab order", async () => {
      await driver.pressPaletteOpenChord();
      await driver.activatePaletteCommand("New module");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(true);
      await expect.poll(async () => driver.isCommandPaletteOpen(), { timeout: 15_000 }).toBe(false);
      const sequence = await driver.modulesXDialogTabSequence();
      // Focus settles on the page body in this entry flow (the palette's
      // close restores it after the dialog mounts); Tab then enters the
      // dialog at the title field, walks the fields in form order, and ends
      // on the Cancel/Submit pair. (Observed: body, Title, Description,
      // dates, status, lead, members, Cancel, Create Module.)
      const titleAt = sequence.findIndex((stop) => stop === "input:Title");
      expect(titleAt).toBeGreaterThanOrEqual(0);
      expect(titleAt).toBeLessThanOrEqual(2);
      const after = (index: number, pattern: RegExp): number => {
        const found = sequence.findIndex((stop, at) => at > index && pattern.test(stop));
        expect(found).toBeGreaterThan(index);
        return found;
      };
      let at = titleAt;
      at = after(at, /description/i);
      at = after(at, /start date|end date|date/i);
      at = after(at, /backlog|status/i);
      at = after(at, /lead/i);
      at = after(at, /members/i);
      const cancelAt = after(at, /^button:Cancel$/);
      expect(sequence[cancelAt + 1]).toBe("button:Create Module");
      await driver.modulesXPressKey("Escape");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(false);
    });

    await test.step("the palette two-key creation shortcut opens the dialog", async () => {
      // Fresh page: click-activating a palette command poisons project
      // sequences (bug NEWFRONT-333, pinned below), so prove the row's
      // claim from clean state.
      await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
      await driver.modulesXPressKeySequence("nm");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(true);
      await driver.modulesXPressKey("Escape");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(false);
    });

    await test.step("a representative set of other keypresses does nothing", async () => {
      await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
      const pathBefore = await driver.currentPath();
      for (const key of ["x", "z", "q", "t", "u", "e", "b", "y"]) {
        await driver.modulesXPressKey(key);
        expect(await driver.modulesXModuleDialogOpen()).toBe(false);
        expect(await driver.isCommandPaletteOpen()).toBe(false);
        expect(await driver.currentPath()).toBe(pathBefore);
        expect(await driver.modulesXListNames()).toEqual([moduleName]);
      }
      // Shift+m belongs to the work-item context; on the modules list it
      // opens nothing either.
      await driver.modulesXPressKey("Shift+m");
      expect(await driver.isCommandPaletteOpen()).toBe(false);
      const rows = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      expect(rows.map((row) => row.name)).toEqual([moduleName]);
    });
  }
);

test(
  specTitle(
    ROW_040,
    "bug: NEWFRONT-333 click-activating a palette command kills nm until navigation (intended: nm keeps working)"
  ),
  { tag: specTags(ROW_040) },
  async ({ driver }) => {
    const world = await setupCrosscuttingWorld("modx-040bug", true);
    const moduleName = `Keys bug ${uniqueSuffixForProjects()}`;
    await serverCreateModule(world.workspaceSlug, world.projectId, moduleName, world.owner.cookie);
    await enterModulesListWith(driver, world, world.owner, moduleName);

    await driver.pressPaletteOpenChord();
    await driver.activatePaletteCommand("New module");
    await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(true);
    await driver.modulesXPressKey("Escape");
    await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(false);

    // Actual: nm no longer opens the dialog; only a navigation heals it.
    // (Workspace sequences like gh keep working throughout, so the
    // handler itself is alive — the project flag half of its context is
    // what breaks.) Intended (row): nm keeps working regardless of prior
    // palette use.
    await driver.modulesXPressKeySequence("nm");
    await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
    expect(await driver.modulesXModuleDialogOpen()).toBe(false);

    await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
    await driver.modulesXPressKeySequence("nm");
    await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(true);
  }
);

test(
  specTitle(ROW_041, "rows and cards ignore drags; no export, import, or print action exists"),
  { tag: specTags(ROW_041) },
  async ({ driver }) => {
    const world = await setupCrosscuttingWorld("modx-041", true);
    const suffix = uniqueSuffixForProjects();
    const names = [`Drag alpha ${suffix}`, `Drag beta ${suffix}`, `Drag gamma ${suffix}`];
    for (const name of names) {
      await serverCreateModule(world.workspaceSlug, world.projectId, name, world.owner.cookie);
    }

    await test.step("dragging list rows changes neither the order nor the server", async () => {
      await enterModulesListWith(driver, world, world.owner, names[0] ?? "");
      const uiBefore = await driver.modulesXListNames();
      const serverBefore = (await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).map(
        (row) => row.name
      );
      await driver.modulesXAttemptRowDrag(uiBefore[0] ?? "", uiBefore[uiBefore.length - 1] ?? "");
      await driver.modulesXAttemptRowDrag(uiBefore[uiBefore.length - 1] ?? "", uiBefore[0] ?? "");
      expect(await driver.modulesXListNames()).toEqual(uiBefore);
      const serverAfter = (await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).map(
        (row) => row.name
      );
      expect(serverAfter).toEqual(serverBefore);
    });

    await test.step("dragging board cards changes neither the order nor the server", async () => {
      await driver.modulesXSwitchModulesLayout("board");
      const uiBefore = await driver.modulesXBoardNames();
      expect(uiBefore.sort()).toEqual([...names].sort());
      await driver.modulesXAttemptCardDrag(uiBefore[0] ?? "", uiBefore[uiBefore.length - 1] ?? "");
      expect(await driver.modulesXBoardNames()).toEqual(uiBefore);
      const serverAfter = (await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).map(
        (row) => row.name
      );
      expect(serverAfter.sort()).toEqual([...names].sort());
      await driver.modulesXSwitchModulesLayout("list");
    });

    await test.step("no export, import, or print action exists on list, detail, or dialogs", async () => {
      expect(await driver.modulesXExportActionsPresent("list")).toBe(false);
      const entries = await driver.modulesXRowMenuEntries(names[0] ?? "");
      expect(entries.length).toBeGreaterThan(0);
      expect(entries.filter((entry) => /export|import|print/i.test(entry))).toEqual([]);
      const rows = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      const firstId = rows.find((row) => row.name === names[0])?.id ?? "";
      await driver.modulesXOpenModuleDetail(world.workspaceSlug, world.projectId, firstId);
      expect(await driver.modulesXExportActionsPresent("detail")).toBe(false);
      await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
      await driver.pressPaletteOpenChord();
      await driver.activatePaletteCommand("New module");
      await expect.poll(async () => driver.modulesXModuleDialogOpen(), { timeout: 15_000 }).toBe(true);
      expect(await driver.modulesXExportActionsPresent("dialog")).toBe(false);
      await driver.modulesXPressKey("Escape");
    });
  }
);

test(
  specTitle(ROW_042, "desktop build serves the same module pages with no desktop-only behavior"),
  { tag: specTags(ROW_042) },
  async ({ driver }) => {
    // Method (recorded in the row): the scratch stack serves one build, so
    // the scenario pins the observable half — the same routes render, list
    // entry talks only to the known module endpoints, and no desktop-shell
    // markers appear — while the inventory records the source sweep (no
    // module code in the desktop shell or edition overlays). Any desktop
    // gate, stub, or extra endpoint fails the render or the allowlist.
    const world = await setupCrosscuttingWorld("modx-042", true);
    const moduleName = `Desktop module ${uniqueSuffixForProjects()}`;
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, moduleName, world.owner.cookie);

    await test.step("the same list and detail routes render with server agreement", async () => {
      await enterModulesListWith(driver, world, world.owner, moduleName);
      await driver.modulesXOpenModuleDetail(world.workspaceSlug, world.projectId, moduleId);
      expect(await driver.modulesXDetailName()).toBe(moduleName);
      expect(await driver.modulesXAnalyticsSidebarVisible()).toBe(true);
      const detail = await modulesXModuleDetail(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      expect(detail.name).toBe(moduleName);
    });

    await test.step("list entry talks only to the known module endpoints", async () => {
      const paths = await driver.modulesXEntryRequestPaths(world.workspaceSlug, world.projectId);
      const listPath = `/api/workspaces/${world.workspaceSlug}/projects/${world.projectId}/modules/`;
      expect(paths.map((path) => path.replace(/\/+$/, ""))).toContain(listPath.replace(/\/+$/, ""));
      // The spy returns every API path, so an edition- or desktop-owned
      // endpoint reached on entry is reported here rather than hidden.
      expect(entryTrafficViolations(paths)).toEqual({ prohibited: [], unknownModule: [] });
    });

    await test.step("no desktop-shell markers appear on the module surfaces", async () => {
      await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
      expect(await driver.modulesXEditionGateMarkersPresent("list")).toBe(false);
      await driver.modulesXOpenModuleDetail(world.workspaceSlug, world.projectId, moduleId);
      expect(await driver.modulesXEditionGateMarkersPresent("detail")).toBe(false);
    });
  }
);

test(
  specTitle(ROW_043, "favorites, filters, timeline, analytics, links, and archives behave identically"),
  { tag: specTags(ROW_043) },
  async ({ driver }) => {
    // Same method as MOD-042: every listed surface renders ungated and list
    // entry stays inside the known module endpoints. Any cloud-only or
    // OSS-only gate, stub, or forked endpoint fails the render, the marker
    // scan, or the allowlist.
    const world = await setupCrosscuttingWorld("modx-043", true);
    const suffix = uniqueSuffixForProjects();
    const firstName = `Edition first ${suffix}`;
    const archivedName = `Edition archived ${suffix}`;
    const firstId = await serverCreateModule(world.workspaceSlug, world.projectId, firstName, world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      firstId,
      { status: "in-progress", start_date: "2026-09-01", target_date: "2026-12-31" },
      world.owner.cookie
    );
    const archivedId = await serverCreateModule(world.workspaceSlug, world.projectId, archivedName, world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      archivedId,
      { status: "completed" },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, archivedId, world.owner.cookie);
    const issueId = await serverCreateIssue(
      world.workspaceSlug,
      world.projectId,
      world.owner.cookie,
      `Edition item ${suffix}`
    );
    await serverAddIssuesToModule(world.workspaceSlug, world.projectId, firstId, [issueId], world.owner.cookie);

    await test.step("favorites and filters render ungated on the list", async () => {
      await enterModulesListWith(driver, world, world.owner, firstName);
      expect(await driver.modulesXRowFavoriteToggleVisible()).toBe(true);
      expect(await driver.modulesXFiltersMenuOpens()).toBe(true);
      await driver.modulesXPressKey("Escape");
      expect(await driver.modulesXEditionGateMarkersPresent("list")).toBe(false);
    });

    await test.step("the timeline renders bars for dated modules", async () => {
      await driver.modulesXSwitchModulesLayout("timeline");
      await expect.poll(async () => driver.modulesXTimelineBarsPresent(), { timeout: 30_000 }).toBe(true);
      await driver.modulesXSwitchModulesLayout("list");
      await expect.poll(async () => driver.modulesXListNames(), { timeout: 30_000 }).toContain(firstName);
    });

    await test.step("analytics and links render ungated on the detail", async () => {
      await driver.modulesXOpenModuleDetail(world.workspaceSlug, world.projectId, firstId);
      expect(await driver.modulesXDetailName()).toBe(firstName);
      expect(await driver.modulesXAnalyticsSidebarVisible()).toBe(true);
      expect(await driver.modulesXLinksSectionVisible()).toBe(true);
      expect(await driver.modulesXEditionGateMarkersPresent("detail")).toBe(false);
    });

    await test.step("the archived-modules screen renders the archived row", async () => {
      await driver.modulesXOpenModulesList(world.workspaceSlug, world.projectId);
      await driver.archivesOpenModulesTab(world.workspaceSlug, world.projectId);
      await expect.poll(async () => driver.archivesModuleRowNames(), { timeout: 30_000 }).toContain(archivedName);
      const archived = await archivesArchivedModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      expect(archived.map((row) => row.name)).toContain(archivedName);
    });

    await test.step("list entry talks only to the known module endpoints", async () => {
      const paths = await driver.modulesXEntryRequestPaths(world.workspaceSlug, world.projectId);
      expect(entryTrafficViolations(paths)).toEqual({ prohibited: [], unknownModule: [] });
    });
  }
);
