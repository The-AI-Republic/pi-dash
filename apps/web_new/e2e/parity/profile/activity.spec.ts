// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-425): member profile activity tab.
// Row PROF-007 (paged feed plus today's-activity CSV download gated to
// admin/member). Green on apps/web first.
//
// Fixture shape: each scenario mints its own owner and workspace, drains
// the backend seed, and creates exactly the project and work items it
// needs. The paging scenario has its member create past one page of
// activity, then reads the server totals to drive the load-more loop, so
// reruns on one seeded stack stay green regardless of stray activity.
//
// Live-verified notes (oracle-prod): the feed pages 100 entries at a
// time through a load-more control that renders only while further pages
// exist; the download control sends the current day and saves the
// server's CSV bytes under a timestamped name, showing a working label
// while fetching. Guests open the same feed view but never see the
// download control (the layout guard denies only the issue-tab
// addresses; see row PROF-002).
import { test, expect } from "../fixtures";
import {
  ROLE,
  PROJECT_ROLE_MEMBER,
  addProjectMembersViaApi,
  browserSessionCookies,
  createIssue,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  parityProjectIdentifier,
  profileActivityExportCsv,
  seatFreshMember,
  serverPatchUser,
  serverUserActivity,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_007 = ["PROF-007"];

interface ActivityWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  workspaceName: string;
  projectId: string;
  projectName: string;
  tag: string;
}

/** Fresh owner plus workspace with the backend seed drained (zero projects). */
async function setupActivityWorld(prefix: string): Promise<ActivityWorld> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const workspaceName = `Prof Act WS ${tag}`;
  const ws = await createWorkspaceForProjects(owner, { name: workspaceName, slug: `pact-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  await drainWorkspaceSeed(owner, ws.slug, workspaceName);
  const projectName = `Prof Act Proj ${tag}`;
  const projectId = await createProjectViaApi(owner, ws.slug, {
    name: projectName,
    identifier: parityProjectIdentifier("PA"),
  });
  return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, workspaceName, projectId, projectName, tag };
}

/** Seat a workspace user at `role`, ready to browse (and optionally to act in the project). */
async function seatActivityUser(
  world: ActivityWorld,
  role: number,
  prefix: string,
  inProject: boolean
): Promise<AuthedSession> {
  const user = await seatFreshMember(world.owner, world.workspaceSlug, role, prefix);
  await markOnboardedForProjects(user);
  await setLastWorkspaceForProjects(user, world.workspaceId);
  if (inProject) {
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
      { member_id: user.userId, role: PROJECT_ROLE_MEMBER },
    ]);
  }
  return user;
}

/** Today's date in the payload shape the download control sends (yyyy-mm-dd, local). */
function todayLocal(): string {
  const now = new Date();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${now.getFullYear()}-${month}-${day}`;
}

test(
  specTitle(ROW_007, "activity tab pages entries and links them into work items"),
  { tag: specTags(ROW_007) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupActivityWorld("parity-prof007");
    const member = await seatActivityUser(world, ROLE.MEMBER, "parity-prof007m", true);
    await serverPatchUser(member.cookie, { display_name: `Opal Seven ${world.tag}` });
    const first = `Prof act first ${world.tag}`;
    const last = `Prof act last ${world.tag}`;
    await createIssue(world.workspaceSlug, world.projectId, member.cookie, first);
    for (let i = 0; i < 104; i++) {
      await createIssue(world.workspaceSlug, world.projectId, member.cookie, `Prof act fill ${world.tag} ${i}`);
    }
    await createIssue(world.workspaceSlug, world.projectId, member.cookie, last);
    const quiet = await seatActivityUser(world, ROLE.MEMBER, "parity-prof007q", false);

    await test.step("the first page renders with load-more while further pages exist", async () => {
      // Activity rows land through the worker queue, so the total lags
      // the creates; wait for the full page-plus-one instead of reading once.
      await expect
        .poll(
          async () =>
            (await serverUserActivity(world.owner, world.workspaceSlug, member.userId, { per_page: 100 }))
              .total_results,
          { timeout: 180_000 }
        )
        .toBeGreaterThan(100);
      await driver.profileActivityOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      const entries = await driver.profileActivityEntries();
      expect(entries).toHaveLength(100);
      expect(entries.join("\n")).toContain(`Opal Seven ${world.tag}`);
      expect(await driver.profileActivityLoadMoreVisible()).toBe(true);
      const hrefs = await driver.profileActivityEntryHrefs();
      expect(hrefs.length).toBeGreaterThan(0);
      for (const href of hrefs) expect(href).toContain("/browse/");
    });

    await test.step("load-more appends the next page and retires on the last one", async () => {
      const server = await serverUserActivity(world.owner, world.workspaceSlug, member.userId, { per_page: 100 });
      for (let guard = 0; guard < 4 && (await driver.profileActivityLoadMoreVisible()); guard++) {
        await driver.profileActivityLoadMore();
      }
      expect(await driver.profileActivityLoadMoreVisible()).toBe(false);
      const entries = await driver.profileActivityEntries();
      expect(entries).toHaveLength(server.total_results);
      expect(entries.join("\n")).toContain(first);
      expect(entries.join("\n")).toContain(last);
    });

    await test.step("a member with no activity sees no entries and no load-more", async () => {
      const server = await serverUserActivity(world.owner, world.workspaceSlug, quiet.userId, { per_page: 100 });
      expect(server.total_results).toBe(0);
      await driver.profileActivityOpen(world.workspaceSlug, quiet.userId, browserSessionCookies(world.owner));
      expect(await driver.profileActivityEntries()).toEqual([]);
      expect(await driver.profileActivityLoadMoreVisible()).toBe(false);
    });
  }
);

test(
  specTitle(ROW_007, "activity download fetches today's CSV with progress, gated to admin and member"),
  { tag: specTags(ROW_007) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupActivityWorld("parity-prof007d");
    const member = await seatActivityUser(world, ROLE.MEMBER, "parity-prof007dm", true);
    const guest = await seatActivityUser(world, ROLE.GUEST, "parity-prof007dg", false);
    await createIssue(world.workspaceSlug, world.projectId, member.cookie, `Prof act down ${world.tag}`);

    await test.step("the control works while fetching, then saves the server's bytes", async () => {
      await driver.profileActivityOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileActivityDownloadVisible()).toBe(true);
      expect(await driver.profileActivityDownloadLabel()).toBe("Download today's activity");

      await driver.profileActivityStallExport();
      const pending = driver.profileActivityDownload();
      await expect.poll(() => driver.profileActivityDownloadLabel(), { timeout: 30_000 }).toBe("Downloading");
      await driver.profileActivityReleaseExport();
      const { file, requestBody } = await pending;

      expect(requestBody.date).toBe(todayLocal());
      expect(await driver.profileActivityDownloadLabel()).toBe("Download today's activity");
      expect(file.fileName).toMatch(/^profile-activity-\d+\.csv$/);

      const serverCsv = await profileActivityExportCsv(
        world.owner,
        world.workspaceSlug,
        member.userId,
        requestBody.date
      );
      expect(file.content).toBe(serverCsv);
      const header = file.content.split(/\r?\n/)[0] ?? "";
      expect(header).toBe(
        '"Actor name","Issue ID","Project","Created at","Updated at","Action","Field","Old value","New value"'
      );
      expect(file.content).toContain(world.projectName);
    });

    await test.step("a member sees the control on another member's activity", async () => {
      await driver.profileActivityOpen(world.workspaceSlug, world.owner.userId, browserSessionCookies(member));
      expect(await driver.profileActivityDownloadVisible()).toBe(true);
      expect(await driver.profileActivityDownloadLabel()).toBe("Download today's activity");
    });

    await test.step("a guest never sees the download control", async () => {
      await driver.profileActivityOpen(world.workspaceSlug, member.userId, browserSessionCookies(guest));
      expect(await driver.profileActivityDownloadVisible()).toBe(false);
      expect(await driver.profileActivityDownloadLabel()).toBeNull();
    });
  }
);
