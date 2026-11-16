// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-381): workspace issue exports settings.
// Rows EXP-001 (page frame for admin/member), EXP-002 (guest refusal,
// non-member no-workspace view), EXP-003 (project picker), EXP-004 (format
// picker), EXP-005 (submit), EXP-006 (history join), EXP-007 (row content),
// EXP-008 (processing states), EXP-009 (live refresh), EXP-010
// (pagination), EXP-011 (download control), EXP-012 (seven-day expiry),
// EXP-013 (skeleton and empty states). Green on apps/web first.
//
// Fixture shape: every scenario mints its own owner and workspace, drains
// the backend seed, and creates exactly the projects it needs (project
// names never reuse the workspace name or contain hyphens, which the
// project-name validator refuses). Real submissions go through the UI
// against the live stack; fixtures the UI cannot produce on demand (bulk
// history, aged records, stuck in-flight rows, terminal failure rows) are
// planted server-side and labeled PLANTED below.
//
// Scratch-stack note: the stack worker has no object-storage env, so the
// background file build always ends failed on scratch and a real export
// never completes there. Completion, download, and expiry cells are
// therefore proven with planted records (the download control points at a
// data: URL so the new-tab proof stays deterministic); the request path,
// toasts, and history join are proven with real UI submissions.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  ROLE,
  addProjectMembersViaApi,
  analyticsWorkspaceName,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  exportsListHistory,
  exportsPatchRecord,
  exportsPlantRecord,
  markOnboardedForProjects,
  parityProjectIdentifier,
  projectsDetails,
  seatFreshMember,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { parityOracleUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";

const ROW_001 = ["EXP-001"];
const ROW_002 = ["EXP-002"];
const ROW_003 = ["EXP-003"];
const ROW_004 = ["EXP-004"];
const ROW_005 = ["EXP-005"];
const ROW_006 = ["EXP-006"];
const ROW_007 = ["EXP-007"];
const ROW_008 = ["EXP-008"];
const ROW_009 = ["EXP-009"];
const ROW_010 = ["EXP-010"];
const ROW_011 = ["EXP-011"];
const ROW_012 = ["EXP-012"];
const ROW_013 = ["EXP-013"];

interface ExportsWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  workspaceName: string;
  tag: string;
}

/** Fresh owner plus workspace with the backend seed drained (zero projects). */
async function setupExportsWorld(prefix: string): Promise<ExportsWorld> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const workspaceName = `Exp WS ${tag}`;
  const ws = await createWorkspaceForProjects(owner, { name: workspaceName, slug: `exp-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  await drainWorkspaceSeed(owner, ws.slug, workspaceName);
  return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, workspaceName, tag };
}

/** Create one project in the world (names stay hyphen-free for the validator). */
async function setupExportsProject(
  world: ExportsWorld,
  name: string,
  identifierPrefix: string
): Promise<{ id: string; name: string; identifier: string }> {
  const identifier = parityProjectIdentifier(identifierPrefix);
  const id = await createProjectViaApi(world.owner, world.workspaceSlug, { name, identifier });
  return { id, name, identifier };
}

/** Seat a workspace user at `role`, optionally onto projects, ready to browse. */
async function seatExportsUser(
  world: ExportsWorld,
  role: number,
  prefix: string,
  projectIds: string[] = []
): Promise<AuthedSession> {
  const user = await seatFreshMember(world.owner, world.workspaceSlug, role, prefix);
  await markOnboardedForProjects(user);
  await setLastWorkspaceForProjects(user, world.workspaceId);
  for (const projectId of projectIds) {
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, projectId, [{ member_id: user.userId, role }]);
  }
  return user;
}

/** Open the exports settings page authenticated as `session`. */
async function enterExportsSettings(
  driver: ParityDriver,
  world: ExportsWorld,
  session: AuthedSession = world.owner
): Promise<void> {
  await driver.exportsSettingsOpen(world.workspaceSlug, browserSessionCookies(session));
}

/** Trailing slashes vary by navigation kind; comparisons ignore them. */
function normalizedPath(path: string): string {
  const withoutQuery = path.split("?", 1)[0] ?? path;
  return withoutQuery.length > 1 ? withoutQuery.replace(/\/+$/, "") : withoutQuery;
}

test(
  specTitle(ROW_001, "exports settings page renders for admins and members"),
  { tag: specTags(ROW_001) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp001");
    const project = await setupExportsProject(world, `Exp One ${world.tag}`, "EX");
    const member = await seatExportsUser(world, ROLE.MEMBER, "parity-exp001m", [project.id]);

    await enterExportsSettings(driver, world);

    await test.step("the owner sees the heading, explanatory text, breadcrumb, and address", async () => {
      expect(await driver.exportsSettingsAccess()).toBe("form");
      expect(await driver.exportsSettingsHeading()).toEqual({
        title: "Exports",
        description: "Export your project data in various formats and access your export history with download links.",
      });
      expect(await driver.exportsSettingsBreadcrumb()).toContain("Exports");
      expect(normalizedPath(await driver.exportsSettingsCurrentPath())).toBe(
        `/${world.workspaceSlug}/settings/exports`
      );
    });

    await test.step("the browser tab title names the current workspace", async () => {
      const serverName = await analyticsWorkspaceName(world.owner, world.workspaceSlug);
      expect(serverName).toBe(world.workspaceName);
      expect(await driver.exportsSettingsPageTitle()).toBe(`${serverName} - Exports`);
    });

    await test.step("a workspace member sees the same working form", async () => {
      await enterExportsSettings(driver, world, member);
      expect(await driver.exportsSettingsAccess()).toBe("form");
      expect(await driver.exportsSettingsHeading()).toEqual({
        title: "Exports",
        description: "Export your project data in various formats and access your export history with download links.",
      });
      expect(await driver.exportsSettingsPageTitle()).toBe(`${world.workspaceName} - Exports`);
    });
  }
);

test(
  specTitle(ROW_002, "guests are refused and non-members land on the no-workspace view"),
  { tag: specTags(ROW_002) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp002");
    await setupExportsProject(world, `Exp One ${world.tag}`, "EX");
    const guest = await seatExportsUser(world, ROLE.GUEST, "parity-exp002g");
    const outsider = await signUpAuthedSession("parity-exp002o");
    await markOnboardedForProjects(outsider);

    await test.step("a workspace guest sees the not-authorized view instead of the form", async () => {
      await enterExportsSettings(driver, world, guest);
      expect(await driver.exportsSettingsAccess()).toBe("not-authorized");
      expect(normalizedPath(await driver.exportsSettingsCurrentPath())).toBe(
        `/${world.workspaceSlug}/settings/exports`
      );
    });

    await test.step("a signed-in user with no workspace standing sees the no-workspace view", async () => {
      await enterExportsSettings(driver, world, outsider);
      expect(await driver.exportsSettingsAccess()).toBe("no-workspace");
      expect(normalizedPath(await driver.exportsSettingsCurrentPath())).toBe(
        `/${world.workspaceSlug}/settings/exports`
      );
    });
  }
);

test(
  specTitle(ROW_003, "project picker lists only creatable projects, searches, and compacts the pick"),
  { tag: specTags(ROW_003) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp003");
    const projectA = await setupExportsProject(world, `Exp Alpha ${world.tag}`, "EA");
    const projectB = await setupExportsProject(world, `Exp Beta ${world.tag}`, "EB");
    // Member of A only: B exists on the server but must stay out of the picker.
    const member = await seatExportsUser(world, ROLE.MEMBER, "parity-exp003m", [projectA.id]);
    expect((await projectsDetails(world.owner, world.workspaceSlug)).map((p) => p.name).sort()).toEqual(
      [projectA.name, projectB.name].sort()
    );

    await enterExportsSettings(driver, world, member);

    await test.step("the picker defaults to all projects and lists only the creatable one", async () => {
      expect(await driver.exportsSettingsProjectPickerLabel()).toBe("All projects");
      expect(await driver.exportsSettingsProjectPickerDisabled()).toBe(false);
      await driver.exportsSettingsProjectPickerOpen();
      expect(await driver.exportsSettingsProjectPickerOptions()).toEqual([
        { key: projectA.identifier, name: projectA.name },
      ]);
    });

    await test.step("search matches names and keys, and misses stay empty", async () => {
      await driver.exportsSettingsProjectPickerSearch(projectB.name.slice(0, 12));
      expect(await driver.exportsSettingsProjectPickerOptions()).toEqual([]);
      await driver.exportsSettingsProjectPickerSearch(projectA.identifier.slice(0, 4));
      await expect
        .poll(() => driver.exportsSettingsProjectPickerOptions(), { timeout: 30_000 })
        .toEqual([{ key: projectA.identifier, name: projectA.name }]);
      await driver.exportsSettingsProjectPickerSearch("");
      await expect
        .poll(() => driver.exportsSettingsProjectPickerOptions(), { timeout: 30_000 })
        .toEqual([{ key: projectA.identifier, name: projectA.name }]);
    });

    await test.step("choosing a project compacts to its key; clearing restores all", async () => {
      await driver.exportsSettingsProjectPickerToggle(projectA.name);
      await expect
        .poll(() => driver.exportsSettingsProjectPickerLabel(), { timeout: 30_000 })
        .toBe(projectA.identifier);
      await driver.exportsSettingsProjectPickerToggle(projectA.name);
      await expect.poll(() => driver.exportsSettingsProjectPickerLabel(), { timeout: 30_000 }).toBe("All projects");
    });

    await test.step("a member who may create nowhere keeps an enabled but empty picker", async () => {
      // Verified live: the picker disables only for below-member workspace
      // tiers, who never reach the form (EXP-002), so a member with no
      // project standing gets an enabled picker with no options.
      const noCreate = await seatExportsUser(world, ROLE.MEMBER, "parity-exp003n");
      await enterExportsSettings(driver, world, noCreate);
      expect(await driver.exportsSettingsAccess()).toBe("form");
      expect(await driver.exportsSettingsProjectPickerDisabled()).toBe(false);
      expect(await driver.exportsSettingsProjectPickerLabel()).toBe("All projects");
      await driver.exportsSettingsProjectPickerOpen();
      expect(await driver.exportsSettingsProjectPickerOptions()).toEqual([]);
    });
  }
);

test(
  specTitle(ROW_004, "format picker offers three formats and the choice labels the history row"),
  { tag: specTags(ROW_004) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp004");
    await setupExportsProject(world, `Exp One ${world.tag}`, "EX");

    await enterExportsSettings(driver, world);
    await driver.exportsSettingsCaptureSubmit();

    await test.step("three formats are offered with CSV first", async () => {
      expect(await driver.exportsSettingsFormatLabel()).toBe("CSV");
      expect(await driver.exportsSettingsFormatOptions()).toEqual(["CSV", "Excel", "JSON"]);
      await driver.exportsSettingsFormatSelect("JSON");
      expect(await driver.exportsSettingsFormatLabel()).toBe("JSON");
    });

    await test.step("the chosen format travels with the request and labels the history row", async () => {
      await driver.exportsSettingsSubmit();
      await expect
        .poll(() => driver.exportsSettingsCapturedSubmits(), { timeout: 60_000 })
        .toEqual([{ provider: "json", project: [], multiple: false }]);
      await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 120_000 }).toBe(1);
      const serverRows = (await exportsListHistory(world.owner, world.workspaceSlug)).results;
      expect(serverRows[0]?.provider).toBe("json");
      const rows = await driver.exportsSettingsHistoryRows();
      expect(rows[0]?.format).toBe("JSON");
    });
  }
);

test(
  specTitle(ROW_005, "submit sends provider projects and the multi flag with working and feedback states"),
  { tag: specTags(ROW_005) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp005");
    const projectA = await setupExportsProject(world, `Exp Alpha ${world.tag}`, "EA");
    const projectB = await setupExportsProject(world, `Exp Beta ${world.tag}`, "EB");

    await enterExportsSettings(driver, world);
    await driver.exportsSettingsCaptureSubmit();

    await test.step("the button works while the request runs and confirms with the format", async () => {
      await driver.exportsSettingsStallSubmit();
      await driver.exportsSettingsSubmit();
      await expect.poll(() => driver.exportsSettingsSubmitLabel(), { timeout: 30_000 }).toBe("Exporting...");
      expect(await driver.exportsSettingsSubmitDisabled()).toBe(true);
      await driver.exportsSettingsReleaseSubmit();
      await expect
        .poll(() => driver.exportsSettingsToast(), { timeout: 60_000 })
        .toEqual({
          kind: "success",
          title: "Export successful",
          message: expect.stringContaining("CSV"),
        });
      expect(await driver.exportsSettingsSubmitLabel()).toBe("Export");
      expect(await driver.exportsSettingsSubmitDisabled()).toBe(false);
      expect(await driver.exportsSettingsCapturedSubmits()).toEqual([
        { provider: "csv", project: [], multiple: false },
      ]);
    });

    await test.step("several projects flag the request for separate files", async () => {
      await driver.exportsSettingsProjectPickerOpen();
      await driver.exportsSettingsProjectPickerToggle(projectA.name);
      await driver.exportsSettingsProjectPickerToggle(projectB.name);
      await driver.exportsSettingsSubmit();
      await expect
        .poll(() => driver.exportsSettingsCapturedSubmits(), { timeout: 60_000 })
        .toEqual([
          { provider: "csv", project: [], multiple: false },
          { provider: "csv", project: expect.arrayContaining([projectA.id, projectB.id]), multiple: true },
        ]);
      const submits = await driver.exportsSettingsCapturedSubmits();
      expect(submits[1]?.project).toHaveLength(2);
    });

    await test.step("a failed submit prompts a retry and the button stays usable", async () => {
      await driver.exportsSettingsFailSubmit();
      await driver.exportsSettingsSubmit();
      await expect
        .poll(() => driver.exportsSettingsToast(), { timeout: 60_000 })
        .toEqual({
          kind: "error",
          title: "Error",
          message: "Export was unsuccessful. Please try again.",
        });
      expect(await driver.exportsSettingsSubmitLabel()).toBe("Export");
      expect(await driver.exportsSettingsSubmitDisabled()).toBe(false);
      await driver.exportsSettingsUnfailSubmit();
      await driver.exportsSettingsSubmit();
      await expect
        .poll(() => driver.exportsSettingsToast(), { timeout: 60_000 })
        .toEqual({
          kind: "success",
          title: "Export successful",
          message: expect.stringContaining("CSV"),
        });
    });
  }
);

test(
  specTitle(ROW_006, "a fresh request joins the history without a manual reload"),
  { tag: specTags(ROW_006) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp006");
    await setupExportsProject(world, `Exp One ${world.tag}`, "EX");

    await enterExportsSettings(driver, world);
    expect(await driver.exportsSettingsHistoryCount()).toBe(0);

    await driver.exportsSettingsSubmit();
    await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 120_000 }).toBe(1);

    await test.step("the server holds the same fresh request", async () => {
      const serverRows = (await exportsListHistory(world.owner, world.workspaceSlug)).results;
      expect(serverRows).toHaveLength(1);
      expect(serverRows[0]?.provider).toBe("csv");
    });
  }
);

test(
  specTitle(ROW_007, "history rows name who when how many in what format and state"),
  { tag: specTags(ROW_007) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp007");
    await setupExportsProject(world, `Exp Alpha ${world.tag}`, "EA");
    await setupExportsProject(world, `Exp Beta ${world.tag}`, "EB");

    await enterExportsSettings(driver, world);
    // Empty choice exports every project the owner belongs to (two here).
    await driver.exportsSettingsSubmit();
    await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 120_000 }).toBe(1);

    await test.step("the row shows who started it, with an initial and no avatar", async () => {
      const rows = await driver.exportsSettingsHistoryRows();
      expect(rows).toHaveLength(1);
      expect(rows[0]?.exportedBy.length ?? 0).toBeGreaterThan(0);
      expect(rows[0]?.avatarUrl).toBeNull();
      expect(rows[0]?.initial?.length ?? 0).toBe(1);
      const serverRows = (await exportsListHistory(world.owner, world.workspaceSlug)).results;
      expect(serverRows[0]?.initiated_by_detail.id).toBe(world.owner.userId);
    });

    await test.step("the row shows when, the project count, the format, and a state", async () => {
      const rows = await driver.exportsSettingsHistoryRows();
      expect(rows[0]?.exportedOn).toMatch(/^[A-Z][a-z]{2} \d{1,2}, \d{4}$/);
      expect(rows[0]?.projectCount).toBe(2);
      expect(rows[0]?.format).toBe("CSV");
      expect(["queued", "processing", "completed", "failed"]).toContain(rows[0]?.status);
    });
  }
);

test(
  specTitle(ROW_008, "in-flight finished failed and waiting exports carry distinct markers"),
  { tag: specTags(ROW_008) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp008");
    const project = await setupExportsProject(world, `Exp One ${world.tag}`, "EX");
    // PLANTED: the UI cannot hold a waiting row or force a failure on
    // demand, so all four states are manufactured server-side.
    await exportsPlantRecord({
      workspaceSlug: world.workspaceSlug,
      ownerEmail: world.owner.email,
      provider: "csv",
      projectIds: [project.id],
      status: "queued",
      createdDaysAgo: 0.004,
    });
    await exportsPlantRecord({
      workspaceSlug: world.workspaceSlug,
      ownerEmail: world.owner.email,
      provider: "xlsx",
      projectIds: [project.id],
      status: "processing",
      createdDaysAgo: 0.003,
    });
    await exportsPlantRecord({
      workspaceSlug: world.workspaceSlug,
      ownerEmail: world.owner.email,
      provider: "json",
      projectIds: [project.id],
      status: "failed",
      createdDaysAgo: 0.002,
    });
    await exportsPlantRecord({
      workspaceSlug: world.workspaceSlug,
      ownerEmail: world.owner.email,
      provider: "csv",
      projectIds: [project.id],
      status: "completed",
      createdDaysAgo: 0.001,
    });

    await enterExportsSettings(driver, world);
    await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 60_000 }).toBe(4);

    const rows = await driver.exportsSettingsHistoryRows();
    // Newest first: completed, failed, processing, queued.
    expect(rows.map((r) => r.status)).toEqual(["completed", "failed", "processing", "queued"]);
    expect(new Set(rows.map((r) => r.status)).size).toBe(4);
    expect(rows.map((r) => r.format)).toEqual(["CSV", "JSON", "Excel", "CSV"]);
  }
);

test(
  specTitle(ROW_009, "the history re-polls while an export runs and offers a manual refresh"),
  { tag: specTags(ROW_009) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp009");
    const project = await setupExportsProject(world, `Exp One ${world.tag}`, "EX");
    // PLANTED: a stuck in-flight row with no background task behind it, so
    // the section must keep re-reading for as long as it is present.
    const stuckId = await exportsPlantRecord({
      workspaceSlug: world.workspaceSlug,
      ownerEmail: world.owner.email,
      provider: "csv",
      projectIds: [project.id],
      status: "processing",
    });

    await enterExportsSettings(driver, world);
    await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 60_000 }).toBe(1);

    await test.step("list reads repeat every few seconds while the export runs", async () => {
      await driver.exportsSettingsWatchHistoryRequests();
      await expect
        .poll(() => driver.exportsSettingsHistoryRequestCount(), { timeout: 30_000 })
        .toBeGreaterThanOrEqual(2);
    });

    await test.step("the manual refresh control re-reads with a working state", async () => {
      expect(await driver.exportsSettingsRefreshLabel()).toBe("Refresh status");
      await driver.exportsSettingsStallHistory();
      await driver.exportsSettingsRefresh();
      await expect.poll(() => driver.exportsSettingsRefreshLabel(), { timeout: 30_000 }).toBe("Refreshing");
      await driver.exportsSettingsReleaseHistory();
      await expect.poll(() => driver.exportsSettingsRefreshLabel(), { timeout: 60_000 }).toBe("Refresh status");
    });

    await test.step("the polling settles once no export is running", async () => {
      await exportsPatchRecord(stuckId, { status: "completed" });
      // Let the in-flight interval land its final read first.
      await driver.page.waitForTimeout(5000);
      const settled = await driver.exportsSettingsHistoryRequestCount();
      await driver.page.waitForTimeout(8000);
      expect(await driver.exportsSettingsHistoryRequestCount()).toBe(settled);
    });
  }
);

test(
  specTitle(ROW_010, "history pages ten records at a time with end-stopped controls"),
  { tag: specTags(ROW_010) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp010");
    const project = await setupExportsProject(world, `Exp One ${world.tag}`, "EX");
    // PLANTED: eleven fresh rows; fractional-day ages keep the newest-first
    // order deterministic without touching the seven-day expiry clock.
    for (let i = 0; i < 11; i += 1) {
      await exportsPlantRecord({
        workspaceSlug: world.workspaceSlug,
        ownerEmail: world.owner.email,
        provider: "csv",
        projectIds: [project.id],
        status: "completed",
        createdDaysAgo: i * 0.001,
      });
    }
    const serverFirst = await exportsListHistory(world.owner, world.workspaceSlug);
    expect(serverFirst.results).toHaveLength(10);
    expect(serverFirst.next_page_results).toBe(true);

    await enterExportsSettings(driver, world);
    await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 60_000 }).toBe(10);

    await test.step("the first page holds ten rows with forward open and back shut", async () => {
      expect(await driver.exportsSettingsPagination()).toEqual({
        visible: true,
        prevDisabled: true,
        nextDisabled: false,
      });
    });

    await test.step("moving forward shows the last row and shuts the forward control", async () => {
      await driver.exportsSettingsGoNext();
      await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 60_000 }).toBe(1);
      expect(await driver.exportsSettingsPagination()).toEqual({
        visible: true,
        prevDisabled: false,
        nextDisabled: true,
      });
    });

    await test.step("moving back restores the first page and shuts the back control", async () => {
      await driver.exportsSettingsGoPrev();
      await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 60_000 }).toBe(10);
      expect(await driver.exportsSettingsPagination()).toEqual({
        visible: true,
        prevDisabled: true,
        nextDisabled: false,
      });
    });
  }
);

test(
  specTitle(ROW_011, "a fresh finished export offers a new-tab download; anything else shows a dash"),
  { tag: specTags(ROW_011) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp011");
    const project = await setupExportsProject(world, `Exp One ${world.tag}`, "EX");
    // PLANTED: one fresh finished row above one stuck in-flight row. The
    // finished row's link points at a same-origin page (the scratch file
    // pipeline cannot complete, so no real file URL exists); the click
    // still proves the control opens its target in a new tab.
    const fileUrl = `${parityOracleUrl(process.env)}/${world.workspaceSlug}/settings/exports`;
    await exportsPlantRecord({
      workspaceSlug: world.workspaceSlug,
      ownerEmail: world.owner.email,
      provider: "csv",
      projectIds: [project.id],
      status: "processing",
      createdDaysAgo: 0.002,
    });
    await exportsPlantRecord({
      workspaceSlug: world.workspaceSlug,
      ownerEmail: world.owner.email,
      provider: "csv",
      projectIds: [project.id],
      status: "completed",
      url: fileUrl,
      createdDaysAgo: 0.001,
    });

    await enterExportsSettings(driver, world);
    await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 60_000 }).toBe(2);

    const rows = await driver.exportsSettingsHistoryRows();
    expect(rows[0]?.download).toEqual({ kind: "link", href: fileUrl, target: "_blank" });
    expect(rows[1]?.download).toEqual({ kind: "dash" });

    await test.step("the control opens the file in a new tab", async () => {
      expect(await driver.exportsSettingsDownloadPopupUrl(0)).toBe(fileUrl);
    });
  }
);

test(
  specTitle(ROW_012, "records older than seven days lose the download control and show an expired marker"),
  { tag: specTags(ROW_012) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp012");
    const project = await setupExportsProject(world, `Exp One ${world.tag}`, "EX");
    // PLANTED: one nine-day-old finished row beside a fresh one. The legacy
    // single-export card has no importer in the old app, so the mounted
    // settings table is the only surface a user can reach; both share the
    // same seven-day cell rule.
    await exportsPlantRecord({
      workspaceSlug: world.workspaceSlug,
      ownerEmail: world.owner.email,
      provider: "csv",
      projectIds: [project.id],
      status: "completed",
      createdDaysAgo: 9,
    });
    await exportsPlantRecord({
      workspaceSlug: world.workspaceSlug,
      ownerEmail: world.owner.email,
      provider: "csv",
      projectIds: [project.id],
      status: "completed",
      url: "data:text/plain,parity-download-probe",
    });

    await enterExportsSettings(driver, world);
    await expect.poll(() => driver.exportsSettingsHistoryCount(), { timeout: 60_000 }).toBe(2);

    const rows = await driver.exportsSettingsHistoryRows();
    expect(rows[0]?.download.kind).toBe("link");
    expect(rows[1]?.download).toEqual({ kind: "expired" });
  }
);

test(
  specTitle(ROW_013, "the history shows a skeleton before the first response and an empty view when bare"),
  { tag: specTags(ROW_013) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupExportsWorld("parity-exp013");

    await test.step("a skeleton placeholder covers the pre-response history", async () => {
      await driver.exportsSettingsStallHistory();
      await enterExportsSettings(driver, world);
      await expect.poll(() => driver.exportsSettingsSkeletonVisible(), { timeout: 60_000 }).toBe(true);
      await driver.exportsSettingsReleaseHistory();
    });

    await test.step("an illustrated empty message explains the bare history", async () => {
      await expect
        .poll(() => driver.exportsSettingsEmptyFacts(), { timeout: 60_000 })
        .toEqual({
          title: "No exports yet",
          description:
            "You don't have any export records right now. Once you export data, all records will appear here.",
        });
      expect(await driver.exportsSettingsHistoryCount()).toBe(0);
      expect(await driver.exportsSettingsSkeletonVisible()).toBe(false);
    });
  }
);
