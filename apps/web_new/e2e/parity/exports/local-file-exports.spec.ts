// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-382): analytics and profile local-file exports.
// Rows EXP-014 (filtered insight-table download), EXP-015 (chart
// drill-down download), EXP-022 (permission-gated profile activity
// download). Green on apps/web first.
//
// Fixture shape: every scenario mints its own owner and workspace, drains
// the backend seed, and creates exactly the projects and work items it
// needs (project names never reuse the workspace name or contain hyphens,
// which the project-name validator refuses). All downloads are files the
// browser builds locally (analytics) or saves from a same-stack API reply
// (profile activity), so no background file build is involved and nothing
// is planted server-side.
//
// Live-verified notes (oracle-prod): the work-items analytics tab renders
// the chart drill-down table first and the insight table second, each with
// its own collapsible search box and "Export as csv" action. Each table's
// search filters its first column and its export covers exactly the shown
// rows; exporting with zero shown rows throws inside the file builder, so
// no file lands. The file builder falls back to a dash placeholder only
// when an export value resolves to nothing, which neither table's value
// functions can produce (they coalesce to empty text or zero before the
// builder runs), so the scenarios pin exact cell equality on complete data
// instead of manufacturing a missing cell.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  ROLE,
  browserSessionCookies,
  createIssue,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  parityProjectIdentifier,
  profileActivityExportCsv,
  seatFreshMember,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_014 = ["EXP-014"];
const ROW_015 = ["EXP-015"];
const ROW_022 = ["EXP-022"];

interface LocalFileWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  workspaceName: string;
  tag: string;
}

/** Fresh owner plus workspace with the backend seed drained (zero projects). */
async function setupLocalFileWorld(prefix: string): Promise<LocalFileWorld> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const workspaceName = `Loc WS ${tag}`;
  const ws = await createWorkspaceForProjects(owner, { name: workspaceName, slug: `loc-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  await drainWorkspaceSeed(owner, ws.slug, workspaceName);
  return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, workspaceName, tag };
}

/** Create one project; retries past identifier collisions on the shared stack. */
async function setupLocalFileProject(
  world: LocalFileWorld,
  name: string,
  identifierPrefix: string
): Promise<{ id: string; name: string; identifier: string }> {
  let id = "";
  let identifier = "";
  for (let attempt = 0; attempt < 4; attempt += 1) {
    identifier = parityProjectIdentifier(identifierPrefix);
    try {
      id = await createProjectViaApi(world.owner, world.workspaceSlug, { name, identifier });
      break;
    } catch (error) {
      if (attempt === 3 || !(error instanceof Error) || !/HTTP 400/.test(error.message)) throw error;
    }
  }
  return { id, name, identifier };
}

/** Seat a workspace user at `role`, ready to browse. */
async function seatLocalFileUser(world: LocalFileWorld, role: number, prefix: string): Promise<AuthedSession> {
  const user = await seatFreshMember(world.owner, world.workspaceSlug, role, prefix);
  await markOnboardedForProjects(user);
  await setLastWorkspaceForProjects(user, world.workspaceId);
  return user;
}

/** Open the analytics export tables authenticated as `session`. */
async function enterAnalyticsExports(
  driver: ParityDriver,
  world: LocalFileWorld,
  session: AuthedSession = world.owner
): Promise<void> {
  await driver.analyticsExportOpen(world.workspaceSlug, browserSessionCookies(session));
}

/** Split CSV text into rows of cells (handles quoted commas and doubled quotes). */
function parseCsvRows(content: string): string[][] {
  const text = content.replace(/^﻿/, "");
  return text
    .split(/\r?\n/)
    .filter((line) => line.length > 0)
    .map((line) => {
      const cells: string[] = [];
      let current = "";
      let inQuotes = false;
      for (let index = 0; index < line.length; index += 1) {
        const ch = line[index];
        if (ch === '"') {
          if (inQuotes && line[index + 1] === '"') {
            current += '"';
            index += 1;
          } else {
            inQuotes = !inQuotes;
          }
        } else if (ch === "," && !inQuotes) {
          cells.push(current);
          current = "";
        } else {
          current += ch;
        }
      }
      cells.push(current);
      return cells;
    });
}

/** Today's date in the payload shape the download control sends (yyyy-mm-dd, local). */
function todayLocal(): string {
  const now = new Date();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${now.getFullYear()}-${month}-${day}`;
}

/** Order-insensitive row comparison for downloads whose row order is server-driven. */
function sortedRows(rows: string[][]): string[] {
  return rows.map((row) => JSON.stringify(row)).sort();
}

test(
  specTitle(ROW_014, "filtered insight-table export saves exactly the filtered rows with export columns"),
  { tag: specTags(ROW_014) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupLocalFileWorld("parity-loc014");
    const alpha = await setupLocalFileProject(world, `Loc Alpha ${world.tag}`, "LA");
    const beta = await setupLocalFileProject(world, `Loc Beta ${world.tag}`, "LB");
    await createIssue(world.workspaceSlug, alpha.id, world.owner.cookie, `Loc issue one ${world.tag}`);
    await createIssue(world.workspaceSlug, alpha.id, world.owner.cookie, `Loc issue two ${world.tag}`);
    await createIssue(world.workspaceSlug, beta.id, world.owner.cookie, `Loc issue three ${world.tag}`);

    await enterAnalyticsExports(driver, world);

    await test.step("both tables render an export action once the analytics data lands", async () => {
      await expect.poll(() => driver.analyticsExportButtonCount(), { timeout: 120_000 }).toBe(2);
      const both = [
        [alpha.name, "2", "0", "0", "0", "0"],
        [beta.name, "1", "0", "0", "0", "0"],
      ];
      await expect
        .poll(async () => sortedRows(await driver.analyticsExportRowCells("insight")), { timeout: 120_000 })
        .toEqual(sortedRows(both));
    });

    await test.step("search narrows the insight table and the file carries exactly that row", async () => {
      await driver.analyticsExportSearch("insight", "Alpha");
      await expect
        .poll(() => driver.analyticsExportRowCells("insight"), { timeout: 30_000 })
        .toEqual([[alpha.name, "2", "0", "0", "0", "0"]]);

      const file = await driver.analyticsExportDownload("insight");
      expect(file.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      expect(parseCsvRows(file.content)).toEqual([
        ["Project", "Backlog", "Started", "Unstarted", "Completed", "Cancelled"],
        [alpha.name, "2", "0", "0", "0", "0"],
      ]);
    });

    await test.step("clearing the search restores both rows to the file", async () => {
      await driver.analyticsExportClearSearch("insight");
      const both = [
        [alpha.name, "2", "0", "0", "0", "0"],
        [beta.name, "1", "0", "0", "0", "0"],
      ];
      await expect
        .poll(async () => sortedRows(await driver.analyticsExportRowCells("insight")), { timeout: 30_000 })
        .toEqual(sortedRows(both));

      const file = await driver.analyticsExportDownload("insight");
      expect(file.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      const rows = parseCsvRows(file.content);
      expect(rows[0]).toEqual(["Project", "Backlog", "Started", "Unstarted", "Completed", "Cancelled"]);
      expect(sortedRows(rows.slice(1))).toEqual(
        sortedRows([
          [alpha.name, "2", "0", "0", "0", "0"],
          [beta.name, "1", "0", "0", "0", "0"],
        ])
      );
    });

    await test.step("a filter with no match exports nothing: the builder throws before any save", async () => {
      await driver.analyticsExportSearch("insight", "zzz-no-such-project");
      await expect.poll(() => driver.analyticsExportRowCells("insight"), { timeout: 30_000 }).toHaveLength(1);
      expect(await driver.analyticsExportYieldsNoDownload("insight")).toBe(true);
    });
  }
);

test(
  specTitle(ROW_015, "chart drill-down export saves the breakdown rows with export columns"),
  { tag: specTags(ROW_015) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupLocalFileWorld("parity-loc015");
    const alpha = await setupLocalFileProject(world, `Loc Alpha ${world.tag}`, "LA");
    const beta = await setupLocalFileProject(world, `Loc Beta ${world.tag}`, "LB");
    await createIssue(world.workspaceSlug, alpha.id, world.owner.cookie, `Loc urgent ${world.tag}`, {
      priority: "urgent",
    });
    await createIssue(world.workspaceSlug, alpha.id, world.owner.cookie, `Loc high ${world.tag}`, {
      priority: "high",
    });
    await createIssue(world.workspaceSlug, beta.id, world.owner.cookie, `Loc low ${world.tag}`, {
      priority: "low",
    });

    await enterAnalyticsExports(driver, world);

    const breakdown = [
      ["High", "1"],
      ["Low", "1"],
      ["Urgent", "1"],
    ];

    await test.step("the drill-down table breaks the work items down by priority", async () => {
      await expect.poll(() => driver.analyticsExportRowCells("chart"), { timeout: 120_000 }).toHaveLength(3);
      expect(sortedRows(await driver.analyticsExportRowCells("chart"))).toEqual(sortedRows(breakdown));
    });

    await test.step("the export saves the breakdown with export columns in a workspace-named file", async () => {
      const file = await driver.analyticsExportDownload("chart");
      expect(file.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      const rows = parseCsvRows(file.content);
      expect(rows[0]).toEqual(["Priority", "Count"]);
      expect(sortedRows(rows.slice(1))).toEqual(sortedRows(breakdown));
    });

    await test.step("the drill-down export follows its search filter like the insight table does", async () => {
      await driver.analyticsExportSearch("chart", "High");
      await expect.poll(() => driver.analyticsExportRowCells("chart"), { timeout: 30_000 }).toEqual([["High", "1"]]);

      const file = await driver.analyticsExportDownload("chart");
      expect(file.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      expect(parseCsvRows(file.content)).toEqual([
        ["Priority", "Count"],
        ["High", "1"],
      ]);
    });
  }
);

test(
  specTitle(ROW_022, "profile activity download is gated, shows work, and saves the server's bytes"),
  { tag: specTags(ROW_022) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupLocalFileWorld("parity-loc022");
    const project = await setupLocalFileProject(world, `Loc Proj ${world.tag}`, "LP");
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, `Loc activity ${world.tag}`);
    const member = await seatLocalFileUser(world, ROLE.MEMBER, "parity-loc022m");
    const guest = await seatLocalFileUser(world, ROLE.GUEST, "parity-loc022g");

    await test.step("the owner sees the idle download control", async () => {
      await driver.profileActivityOpen(world.workspaceSlug, world.owner.userId, browserSessionCookies(world.owner));
      expect(await driver.profileActivityDownloadVisible()).toBe(true);
      expect(await driver.profileActivityDownloadLabel()).toBe("Download today's activity");
    });

    await test.step("the control works while fetching, then saves the server's bytes", async () => {
      await driver.profileActivityStallExport();
      const pending = driver.profileActivityDownload();
      await expect.poll(() => driver.profileActivityDownloadLabel(), { timeout: 30_000 }).toBe("Downloading");
      await driver.profileActivityReleaseExport();
      const { file, requestBody } = await pending;

      expect(requestBody.date).toBe(todayLocal());
      expect(await driver.profileActivityDownloadLabel()).toBe("Download today's activity");

      const stamp = /^profile-activity-(\d+)\.csv$/.exec(file.fileName)?.[1];
      expect(stamp).toBeDefined();
      expect(Date.now() - Number(stamp ?? "0")).toBeLessThan(120_000);

      const serverCsv = await profileActivityExportCsv(
        world.owner,
        world.workspaceSlug,
        world.owner.userId,
        requestBody.date
      );
      expect(file.content).toBe(serverCsv);
      const rows = parseCsvRows(file.content);
      expect(rows[0]).toEqual([
        "Actor name",
        "Issue ID",
        "Project",
        "Created at",
        "Updated at",
        "Action",
        "Field",
        "Old value",
        "New value",
      ]);
      expect(rows.length).toBeGreaterThan(1);
      expect(rows.slice(1).join("\n")).toContain(project.name);
    });

    await test.step("a member sees the control: the gate is workspace tier, not self-only", async () => {
      await driver.profileActivityOpen(world.workspaceSlug, world.owner.userId, browserSessionCookies(member));
      expect(await driver.profileActivityDownloadVisible()).toBe(true);
      expect(await driver.profileActivityDownloadLabel()).toBe("Download today's activity");
    });

    await test.step("a guest never reaches the control", async () => {
      await driver.profileActivityOpen(world.workspaceSlug, world.owner.userId, browserSessionCookies(guest));
      expect(await driver.profileActivityDownloadVisible()).toBe(false);
      expect(await driver.profileActivityDownloadLabel()).toBeNull();
    });
  }
);
