// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-425): member profile pages.
// Rows PROF-001 (open + breadcrumb), PROF-002 (tabs by role),
// PROF-003 (overview count cards), PROF-004 (workload and distribution
// charts), PROF-005 (recent activity), PROF-006 (issue tabs) and PROF-008
// (identity sidebar). Green on apps/web first.
//
// Fixture shape: every scenario mints its own owner and workspace, drains
// the backend seed, and creates exactly the project and work items it
// needs (project names never reuse the workspace name or contain hyphens,
// which the project-name validator refuses). Members join the project
// when they must act inside it; guests stay workspace-only. Reruns on one
// seeded stack stay green through unique names per run.
//
// Live-verified notes (oracle-prod): guests see the summary tab only and
// meet the inline denial on the assigned/created/subscribed addresses,
// while the activity address renders its feed view without a denial and
// without the download control. The desktop tab strip never marks the
// current tab and the mobile picker label always stays empty (bug
// NEWFRONT-433: the tab match paths carry trailing slashes the pathname
// never has). The sidebar still carries cover art and the own-profile
// edit pencil still opens the settings dialog (removal and navigation
// are main#523, main#574 and main#573 targets, pinned as row Gaps).
// Profile layout/display choices persist in browser local storage, so
// persistence is proven by reload inside one scenario context.
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
  seatFreshMember,
  serverPatchUser,
  serverProfileSegregation,
  serverSubscribeIssue,
  serverUploadUserAvatar,
  serverUserActivity,
  serverUserIssues,
  serverUserStats,
  serverWorkspaceMemberRole,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_001 = ["PROF-001"];
const ROW_002 = ["PROF-002"];
const ROW_003 = ["PROF-003"];
const ROW_004 = ["PROF-004"];
const ROW_005 = ["PROF-005"];
const ROW_006 = ["PROF-006"];
const ROW_008 = ["PROF-008"];

interface ProfileWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  workspaceName: string;
  projectId: string;
  projectName: string;
  tag: string;
}

/** Fresh owner plus workspace with the backend seed drained (zero projects). */
async function setupProfileWorld(prefix: string, projectPrefix = "PP"): Promise<ProfileWorld> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const workspaceName = `Prof WS ${tag}`;
  const ws = await createWorkspaceForProjects(owner, { name: workspaceName, slug: `prof-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  await drainWorkspaceSeed(owner, ws.slug, workspaceName);
  const projectName = `Prof Proj ${tag}`;
  const projectId = await createProjectViaApi(owner, ws.slug, {
    name: projectName,
    identifier: parityProjectIdentifier(projectPrefix),
  });
  return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, workspaceName, projectId, projectName, tag };
}

/** Seat a workspace user at `role`, ready to browse (and optionally to act in the project). */
async function seatProfileUser(
  world: ProfileWorld,
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

/** Order-insensitive comparison for server-driven row order. */
function sortedStrings(values: string[]): string[] {
  return [...values].sort();
}

test(
  specTitle(ROW_001, "member profiles open with own/other breadcrumb labels"),
  { tag: specTags(ROW_001) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupProfileWorld("parity-prof001");
    const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof001m", true);
    await serverPatchUser(member.cookie, {
      first_name: "Opal",
      last_name: "Whelan",
      display_name: `Opal Whelan ${world.tag}`,
    });

    await test.step("your own profile labels the breadcrumb as your work", async () => {
      await driver.profileOpen(world.workspaceSlug, world.owner.userId, browserSessionCookies(world.owner));
      expect(await driver.profileBreadcrumbLabel()).toBe("Your work");
      expect(await driver.profileVisibleTabs()).toEqual(["Summary", "Assigned", "Created", "Subscribed", "Activity"]);
      expect(await driver.profileSidebarVisible()).toBe(true);
    });

    await test.step("another member's profile names them and matches the server record", async () => {
      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileBreadcrumbLabel()).toBe("Opal Whelan Work");
      const server = await serverProfileSegregation(world.owner, world.workspaceSlug, member.userId);
      expect(`${server.user_data.first_name} ${server.user_data.last_name} Work`).toBe("Opal Whelan Work");
      expect(server.user_data.display_name).toBe(`Opal Whelan ${world.tag}`);
      const name = await driver.profileSidebarName();
      expect(name.full).toBe("Opal Whelan");
      expect(name.display).toBe(`(Opal Whelan ${world.tag})`);
    });
  }
);

test(
  specTitle(ROW_002, "profile tabs follow workspace role; guests are denied the issue tabs"),
  { tag: specTags(ROW_002) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupProfileWorld("parity-prof002");
    const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof002m", true);
    const guest = await seatProfileUser(world, ROLE.GUEST, "parity-prof002g", false);
    const fullTabs = ["Summary", "Assigned", "Created", "Subscribed", "Activity"];

    await test.step("admins and members see every tab with tab hrefs", async () => {
      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileVisibleTabs()).toEqual(fullTabs);
      expect(await driver.profileTabHref("Summary")).toBe(`/${world.workspaceSlug}/profile/${member.userId}/`);
      expect(await driver.profileTabHref("Assigned")).toBe(
        `/${world.workspaceSlug}/profile/${member.userId}/assigned/`
      );
      expect(await driver.profileTabHref("Activity")).toBe(
        `/${world.workspaceSlug}/profile/${member.userId}/activity/`
      );
      expect(await driver.profileTabHref("Nope")).toBeNull();

      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(member));
      expect(await driver.profileVisibleTabs()).toEqual(fullTabs);
      await driver.profileOpenTab(world.workspaceSlug, member.userId, "assigned", browserSessionCookies(member));
      expect(await driver.profileAccessDenied()).toBe(false);
    });

    await test.step("guests see the summary tab only", async () => {
      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(guest));
      expect(await driver.profileVisibleTabs()).toEqual(["Summary"]);
    });

    await test.step("guests opening an issue tab directly meet the inline denial", async () => {
      for (const tab of ["assigned", "created", "subscribed"]) {
        await driver.profileOpenTab(world.workspaceSlug, member.userId, tab, browserSessionCookies(guest));
        expect(await driver.profileAccessDenied()).toBe(true);
      }
    });

    await test.step("guests opening the activity tab see its feed view without denial or download", async () => {
      await driver.profileOpenTab(world.workspaceSlug, member.userId, "activity", browserSessionCookies(guest));
      expect(await driver.profileAccessDenied()).toBe(false);
      expect(await driver.profileActivityDownloadVisible()).toBe(false);
      expect(await driver.profileActivityDownloadLabel()).toBeNull();
      expect(await driver.profileActivityEntries()).toEqual([]);
    });

    await test.step("the server confirms the three workspace tiers", async () => {
      expect(await serverWorkspaceMemberRole(world.owner, world.workspaceSlug)).toBe(ROLE.ADMIN);
      expect(await serverWorkspaceMemberRole(member, world.workspaceSlug)).toBe(ROLE.MEMBER);
      expect(await serverWorkspaceMemberRole(guest, world.workspaceSlug)).toBe(ROLE.GUEST);
    });
  }
);

test(
  specTitle(ROW_003, "overview count cards carry server totals, link to tabs, and load behind placeholders"),
  { tag: specTags(ROW_003) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupProfileWorld("parity-prof003");
    const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof003m", true);
    await createIssue(world.workspaceSlug, world.projectId, member.cookie, `Prof created ${world.tag}`);
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, `Prof assigned one ${world.tag}`, {
      assignee_ids: [member.userId],
    });
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, `Prof assigned two ${world.tag}`, {
      assignee_ids: [member.userId],
    });
    const extra = await createIssue(
      world.workspaceSlug,
      world.projectId,
      world.owner.cookie,
      `Prof subscribed ${world.tag}`
    );
    await serverSubscribeIssue(member, world.workspaceSlug, world.projectId, extra.id);

    // First: the app caches stats reads in-session, so the placeholder
    // proof must run before any step fetches them.
    await test.step("placeholder blocks show while the stats load", async () => {
      await driver.profileOverviewStallStats();
      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileOverviewStatsLoading()).toBe(true);
      await driver.profileOverviewReleaseStats();
      await expect.poll(() => driver.profileOverviewCards(), { timeout: 60_000 }).toHaveLength(3);
      expect(await driver.profileOverviewStatsLoading()).toBe(false);
    });

    await test.step("cards render the server totals with tab hrefs", async () => {
      const server = await serverUserStats(world.owner, world.workspaceSlug, member.userId);
      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      const cards = await driver.profileOverviewCards();
      expect(cards.map((card) => card.title)).toEqual([
        "Work items created",
        "Work items assigned",
        "Work items subscribed",
      ]);
      expect(cards.map((card) => card.value)).toEqual([
        String(server.created_issues),
        String(server.assigned_issues),
        String(server.subscribed_issues),
      ]);
      expect(server.created_issues).toBe(1);
      expect(server.assigned_issues).toBe(2);
      expect(cards.map((card) => card.href)).toEqual([
        `/${world.workspaceSlug}/profile/${member.userId}/created/`,
        `/${world.workspaceSlug}/profile/${member.userId}/assigned/`,
        `/${world.workspaceSlug}/profile/${member.userId}/subscribed/`,
      ]);
    });

    await test.step("each card navigates to its tab", async () => {
      for (const route of ["created", "assigned", "subscribed"]) {
        await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
        await driver.profileOverviewOpenCard(route);
        expect(await driver.profileAccessDenied()).toBe(false);
        expect(await driver.layoutsListVisible()).toBe(true);
      }
    });
  }
);

/** Workload card label back to its server state group. */
function workloadGroupOf(label: string): string {
  switch (label) {
    case "Not started":
      return "unstarted";
    case "Working on":
      return "started";
    case "Canceled":
      return "cancelled";
    default:
      return label.toLowerCase();
  }
}

test(
  specTitle(ROW_004, "workload and distribution charts render from stats with zeros not gaps"),
  { tag: specTags(ROW_004) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupProfileWorld("parity-prof004");
    const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof004m", true);
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, `Prof urgent ${world.tag}`, {
      assignee_ids: [member.userId],
      priority: "urgent",
    });
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, `Prof medium ${world.tag}`, {
      assignee_ids: [member.userId],
      priority: "medium",
    });
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, `Prof low ${world.tag}`, {
      assignee_ids: [member.userId],
      priority: "low",
    });
    const quiet = await seatProfileUser(world, ROLE.MEMBER, "parity-prof004q", false);

    await test.step("workload cards zero-fill the groups with no items", async () => {
      const server = await serverUserStats(world.owner, world.workspaceSlug, member.userId);
      const counts = new Map(server.state_distribution.map((group) => [group.state_group, group.state_count]));
      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      const groups = await driver.profileWorkloadGroups();
      expect(groups.map((group) => group.label)).toEqual([
        "Backlog",
        "Not started",
        "Working on",
        "Review",
        "Test",
        "Completed",
        "Canceled",
      ]);
      for (const group of groups) {
        expect(group.count).toBe(String(counts.get(workloadGroupOf(group.label)) ?? 0));
      }
      expect(groups.find((group) => group.label === "Backlog")?.count).toBe("3");
    });

    await test.step("priority and state breakdowns follow the stats payload", async () => {
      const server = await serverUserStats(world.owner, world.workspaceSlug, member.userId);
      const categories = await driver.profilePriorityChartCategories();
      const wanted = server.priority_distribution.map(
        (row) => row.priority.slice(0, 1).toUpperCase() + row.priority.slice(1)
      );
      expect(sortedStrings(categories)).toEqual(sortedStrings(wanted));
      const legend = await driver.profileStateChartLegend();
      const counts = new Map(server.state_distribution.map((group) => [group.state_group, group.state_count]));
      expect(legend.map((row) => row.label)).toEqual([
        "Backlog",
        "Unstarted",
        "Started",
        "Review",
        "Test",
        "Completed",
        "Canceled",
      ]);
      for (const row of legend) {
        const group = row.label === "Canceled" ? "cancelled" : row.label.toLowerCase();
        expect(row.count).toBe(String(counts.get(group) ?? 0));
      }
      expect(await driver.profileChartsEmptyCount()).toBe(0);
    });

    await test.step("a member with nothing assigned sees zeros and chart empty states", async () => {
      const server = await serverUserStats(world.owner, world.workspaceSlug, quiet.userId);
      expect(server.assigned_issues).toBe(0);
      await driver.profileOpen(world.workspaceSlug, quiet.userId, browserSessionCookies(world.owner));
      const groups = await driver.profileWorkloadGroups();
      expect(groups).toHaveLength(7);
      for (const group of groups) expect(group.count).toBe("0");
      expect(await driver.profilePriorityChartCategories()).toEqual([]);
      expect(await driver.profileStateChartLegend()).toEqual([]);
      expect(await driver.profileChartsEmptyCount()).toBe(2);
    });
  }
);

test(
  specTitle(ROW_005, "recent activity lists entries with work-item links, or the empty state"),
  { tag: specTags(ROW_005) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupProfileWorld("parity-prof005");
    const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof005m", true);
    await serverPatchUser(member.cookie, { display_name: `Opal Five ${world.tag}` });
    const made = `Prof acted ${world.tag}`;
    await createIssue(world.workspaceSlug, world.projectId, member.cookie, made);
    const quiet = await seatProfileUser(world, ROLE.MEMBER, "parity-prof005q", false);

    await test.step("entries render with links into the underlying work items", async () => {
      const server = await serverUserActivity(world.owner, world.workspaceSlug, member.userId, { per_page: 10 });
      expect(server.total_results).toBeGreaterThan(0);
      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileOverviewActivityEmpty()).toBe(false);
      const entries = await driver.profileOverviewActivityEntries();
      expect(entries.length).toBeGreaterThan(0);
      expect(entries.join("\n")).toContain(made);
      expect(entries.join("\n")).toContain(`Opal Five ${world.tag}`);
      const hrefs = await driver.profileOverviewActivityEntryHrefs();
      expect(hrefs.length).toBeGreaterThan(0);
      for (const href of hrefs) expect(href).toContain("/browse/");
    });

    await test.step("a member with no recent activity sees the compact empty state", async () => {
      const server = await serverUserActivity(world.owner, world.workspaceSlug, quiet.userId, { per_page: 10 });
      expect(server.total_results).toBe(0);
      await driver.profileOpen(world.workspaceSlug, quiet.userId, browserSessionCookies(world.owner));
      expect(await driver.profileOverviewActivityEmpty()).toBe(true);
      expect(await driver.profileOverviewActivityEntries()).toEqual([]);
    });
  }
);

test(
  specTitle(ROW_006, "issue tabs list the matching issues; an unknown tab id renders nothing"),
  { tag: specTags(ROW_006) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupProfileWorld("parity-prof006");
    const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof006m", true);
    const assigned = `Prof tab assigned ${world.tag}`;
    const created = `Prof tab created ${world.tag}`;
    const subscribed = `Prof tab subscribed ${world.tag}`;
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, assigned, {
      assignee_ids: [member.userId],
    });
    await createIssue(world.workspaceSlug, world.projectId, member.cookie, created);
    const extra = await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, subscribed);
    await serverSubscribeIssue(member, world.workspaceSlug, world.projectId, extra.id);

    const tabNames = async (tab: string): Promise<string[]> => {
      await driver.profileOpenTab(world.workspaceSlug, member.userId, tab, browserSessionCookies(world.owner));
      expect(await driver.profileAccessDenied()).toBe(false);
      expect(await driver.layoutsListVisible()).toBe(true);
      const groups = await driver.layoutsListGroups();
      expect(groups).toContain("All work items");
      return driver.layoutsListGroupIssueNames("All work items");
    };

    await test.step("each tab lists exactly its server-side issues", async () => {
      const wantAssigned = await serverUserIssues(world.owner, world.workspaceSlug, member.userId, {
        assignees: member.userId,
      });
      expect(sortedStrings(await tabNames("assigned"))).toEqual(sortedStrings(wantAssigned.map((issue) => issue.name)));
      expect(wantAssigned.map((issue) => issue.name)).toContain(assigned);

      const wantCreated = await serverUserIssues(world.owner, world.workspaceSlug, member.userId, {
        created_by: member.userId,
      });
      expect(sortedStrings(await tabNames("created"))).toEqual(sortedStrings(wantCreated.map((issue) => issue.name)));
      expect(wantCreated.map((issue) => issue.name)).toEqual([created]);

      const wantSubscribed = await serverUserIssues(world.owner, world.workspaceSlug, member.userId, {
        subscriber: member.userId,
      });
      expect(sortedStrings(await tabNames("subscribed"))).toEqual(
        sortedStrings(wantSubscribed.map((issue) => issue.name))
      );
      expect(wantSubscribed.map((issue) => issue.name)).toContain(subscribed);
    });

    await test.step("an unknown tab id renders nothing but keeps the frame", async () => {
      await driver.profileOpenTab(
        world.workspaceSlug,
        member.userId,
        `bogus-${world.tag}`,
        browserSessionCookies(world.owner)
      );
      expect(await driver.profileAccessDenied()).toBe(false);
      expect(await driver.profileTabRendersNothing()).toBe(true);
      expect(await driver.profileVisibleTabs()).toEqual(["Summary", "Assigned", "Created", "Subscribed", "Activity"]);
    });
  }
);

test(
  specTitle(ROW_006, "issue tabs keep layout and display choices; selecting an issue peeks it"),
  { tag: specTags(ROW_006) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupProfileWorld("parity-prof006b", "PQ");
    const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof006n", true);
    const urgent = `Prof persist urgent ${world.tag}`;
    const low = `Prof persist low ${world.tag}`;
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, urgent, {
      assignee_ids: [member.userId],
      priority: "urgent",
    });
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, low, {
      assignee_ids: [member.userId],
      priority: "low",
    });

    await test.step("the switcher offers list and kanban and the choice persists", async () => {
      await driver.profileOpenTab(world.workspaceSlug, member.userId, "assigned", browserSessionCookies(world.owner));
      expect(await driver.profileIssuesOfferedLayouts()).toEqual(["list", "kanban"]);
      expect(await driver.layoutsActiveLayout()).toBe("list");

      await driver.layoutsSwitchTo("kanban");
      expect(await driver.layoutsKanbanVisible()).toBe(true);
      await driver.layoutsReloadIssues();
      expect(await driver.layoutsActiveLayout()).toBe("kanban");

      await driver.layoutsSwitchTo("list");
      expect(await driver.layoutsListVisible()).toBe(true);
      await driver.layoutsReloadIssues();
      expect(await driver.layoutsActiveLayout()).toBe("list");
    });

    await test.step("display choices apply and persist across visits", async () => {
      await driver.openDisplayOptions();
      await driver.setDisplayGroupBy("Priority");
      await driver.closeDisplayOptions();
      await expect
        .poll(async () => sortedStrings(await driver.layoutsListGroups()), { timeout: 30_000 })
        .toEqual(["Low", "Urgent"]);
      const grouped = await driver.layoutsListGroups();
      await driver.layoutsReloadIssues();
      expect(await driver.layoutsListGroups()).toEqual(grouped);

      await driver.openDisplayOptions();
      await driver.setDisplayExtraOption("Show empty groups", true);
      expect(await driver.isDisplayOptionChecked("Show empty groups")).toBe(true);
      await driver.closeDisplayOptions();
      await driver.layoutsReloadIssues();
      await driver.openDisplayOptions();
      expect(await driver.isDisplayOptionChecked("Show empty groups")).toBe(true);
      await driver.setDisplayExtraOption("Show empty groups", false);
      await driver.closeDisplayOptions();
    });

    await test.step("selecting an issue opens its peek overview", async () => {
      await driver.clickListRow(urgent);
      await expect.poll(() => driver.peekTitle(), { timeout: 60_000 }).toBe(urgent);
      expect(await driver.peekOpen()).toBe(true);
      await driver.closePeek();
      expect(await driver.peekOpen()).toBe(false);
    });
  }
);

/** Minutes since midnight of a "HH:MM ..." clock reading. */
function clockMinutes(clock: string): number {
  const match = /^(\d{2}):(\d{2})/.exec(clock);
  if (!match) throw new Error(`[parity] clock reading carries no HH:MM: ${clock}.`);
  return Number(match[1]) * 60 + Number(match[2]);
}

/** Current time in the member's timezone, in the sidebar's HH:MM shape. */
function clockNow(timeZone: string): string {
  return new Intl.DateTimeFormat("en-US", {
    timeZone,
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
  })
    .format(new Date())
    .replace(/^24:/, "00:");
}

test(
  specTitle(ROW_008, "identity sidebar shows avatar, dates, live time and per-project rows"),
  { tag: specTags(ROW_008) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupProfileWorld("parity-prof008");
    const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof008m", true);
    await serverPatchUser(member.cookie, {
      first_name: "Opal",
      last_name: "Whelan",
      display_name: `Opal Whelan ${world.tag}`,
      user_timezone: "Pacific/Kiritimati",
    });
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, `Prof side one ${world.tag}`, {
      assignee_ids: [member.userId],
    });
    await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, `Prof side two ${world.tag}`, {
      assignee_ids: [member.userId],
    });
    const projectTwo = `Prof Proj Two ${world.tag}`;
    const projectTwoId = await createProjectViaApi(world.owner, world.workspaceSlug, {
      name: projectTwo,
      identifier: parityProjectIdentifier("PZ"),
    });
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, projectTwoId, [
      { member_id: member.userId, role: PROJECT_ROLE_MEMBER },
    ]);
    await createIssue(world.workspaceSlug, projectTwoId, world.owner.cookie, `Prof side three ${world.tag}`, {
      assignee_ids: [member.userId],
    });
    const photo = await seatProfileUser(world, ROLE.MEMBER, "parity-prof008p", false);
    // A one-pixel PNG; the bytes only need to satisfy the image upload.
    const pixel = Uint8Array.from([
      137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196,
      137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 0, 0, 0, 2, 0, 1, 225, 27, 205, 133, 0, 0, 0, 0, 73, 69, 78,
      68, 174, 66, 96, 130,
    ]);
    const avatar = await serverUploadUserAvatar(photo, {
      name: `avatar-${world.tag}.png`,
      mime: "image/png",
      bytes: pixel,
    });

    await test.step("cover art, initial fallback, join date and live clock render", async () => {
      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileSidebarVisible()).toBe(true);
      if (driver.target === "web_new") {
        // Target per main#523/main#574 (PROF-008): the panel has no cover.
        expect(await driver.profileSidebarCoverVisible()).toBe(false);
      } else {
        // Current behavior: the cover banner still renders (its removal is
        // the main#523/main#574 target recorded as a Gap in row PROF-008).
        expect(await driver.profileSidebarCoverVisible()).toBe(true);
      }
      expect(await driver.profileSidebarAvatar()).toEqual({ kind: "initial", initial: "O" });

      const server = await serverProfileSegregation(world.owner, world.workspaceSlug, member.userId);
      expect(server.user_data.user_timezone).toBe("Pacific/Kiritimati");
      const joined = await driver.profileSidebarJoinDate();
      expect(joined).toMatch(/^[A-Z][a-z]{2} \d{1,2}, \d{4}$/);
      expect(server.user_data.date_joined.slice(0, 10)).toBe(new Date().toISOString().slice(0, 10));

      const clock = await driver.profileSidebarClock();
      expect(clock).toMatch(/^\d{2}:\d{2} Pacific\/Kiritimati$/);
      const skew = Math.abs(clockMinutes(clock) - clockMinutes(clockNow("Pacific/Kiritimati")));
      expect(Math.min(skew, 1440 - skew)).toBeLessThanOrEqual(1);
    });

    await test.step("the clock ticks in the member's own timezone", async () => {
      const first = await driver.profileSidebarClock();
      await expect.poll(() => driver.profileSidebarClock(), { timeout: 75_000 }).not.toBe(first);
    });

    await test.step("one expandable row per project carries the server counts", async () => {
      const server = await serverProfileSegregation(world.owner, world.workspaceSlug, member.userId);
      const rows = await driver.profileSidebarProjectRows();
      expect(sortedStrings(rows.map((row) => row.name))).toEqual(sortedStrings([world.projectName, projectTwo]));
      for (const row of rows) expect(row.completion).toBe("0%");
      for (const project of server.project_data) {
        const name = project.id === world.projectId ? world.projectName : projectTwo;
        await driver.profileSidebarExpandProject(name);
        expect(await driver.profileSidebarProjectCounts(name)).toEqual({
          created: String(project.created_issues),
          assigned: String(project.assigned_issues),
          due: String(project.pending_issues),
          completed: String(project.completed_issues),
        });
      }
    });

    await test.step("a set avatar photo renders instead of the initial", async () => {
      const server = await serverProfileSegregation(world.owner, world.workspaceSlug, photo.userId);
      expect(server.user_data.avatar_url).toBe(avatar.assetUrl);
      await driver.profileOpen(world.workspaceSlug, photo.userId, browserSessionCookies(world.owner));
      const rendered = await driver.profileSidebarAvatar();
      expect(rendered.kind).toBe("image");
      if (rendered.kind === "image") expect(rendered.src).toContain(avatar.assetId);
    });

    await test.step("only your own profile offers the edit pencil, opening the settings general tab", async () => {
      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileSidebarEditPencilVisible()).toBe(false);

      await driver.profileOpen(world.workspaceSlug, world.owner.userId, browserSessionCookies(world.owner));
      expect(await driver.profileSidebarEditPencilVisible()).toBe(true);
      await driver.profileSidebarClickEditPencil();
      if (driver.target === "web_new") {
        // Target per main#573 (PROF-008): the pencil navigates to the
        // settings general tab instead of opening a dialog.
        await expect.poll(() => driver.layoutsCurrentUrl(), { timeout: 30_000 }).toContain("/settings/profile/general");
        expect(await driver.profileSettingsActiveTab()).toBe("Profile");
      } else {
        // Current behavior: the pencil opens the settings dialog on its
        // general tab (navigation to /settings/profile/general is the
        // main#573 target recorded as a Gap in row PROF-008).
        expect(await driver.profileSettingsActiveTab()).toBe("Profile");
        const url = await driver.layoutsCurrentUrl();
        expect(url).toContain(`/profile/${world.owner.userId}`);
        expect(url).not.toContain("/settings/");
      }
    });
  }
);

test.describe("profile pages on small screens", () => {
  test.use({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
    userAgent:
      "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
  });

  test(
    specTitle(ROW_002, "mobile tab picker offers the role's tabs"),
    { tag: specTags(ROW_002) },
    async ({ driver }) => {
      test.setTimeout(720_000);
      const world = await setupProfileWorld("parity-prof002m");
      const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof002mm", true);
      const guest = await seatProfileUser(world, ROLE.GUEST, "parity-prof002mg", false);

      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileMobileTabPickerOptions()).toEqual([
        "Summary",
        "Assigned",
        "Created",
        "Subscribed",
        "Activity",
      ]);

      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(guest));
      expect(await driver.profileMobileTabPickerOptions()).toEqual(["Summary"]);
    }
  );

  test(
    specTitle(ROW_002, "bug: NEWFRONT-433 mobile tab picker label stays empty on every tab"),
    { tag: specTags(ROW_002) },
    async ({ driver }) => {
      test.setTimeout(720_000);
      const world = await setupProfileWorld("parity-prof002b");
      const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof002bm", true);

      // The old app never matches the current tab, so its picker label
      // stays empty (NEWFRONT-433). web_new builds the target from spec §1:
      // the picker names the current tab, so the target legs assert the
      // labelled picker rather than the pinned defect.
      const labels =
        driver.target === "web_new" ? { summary: "Summary", assigned: "Assigned" } : { summary: "", assigned: "" };

      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileMobileTabPickerLabel()).toBe(labels.summary);
      await driver.profileOpenTab(world.workspaceSlug, member.userId, "assigned", browserSessionCookies(world.owner));
      expect(await driver.profileMobileTabPickerLabel()).toBe(labels.assigned);
    }
  );

  test(
    specTitle(ROW_006, "mobile layout and display menus switch layouts and persist"),
    { tag: specTags(ROW_006) },
    async ({ driver }) => {
      test.setTimeout(720_000);
      const world = await setupProfileWorld("parity-prof006m", "PM");
      const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof006mm", true);
      await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, `Prof mob ${world.tag}`, {
        assignee_ids: [member.userId],
      });

      await driver.profileOpenTab(world.workspaceSlug, member.userId, "assigned", browserSessionCookies(world.owner));
      expect(await driver.profileMobileOfferedLayouts()).toEqual(["list", "kanban"]);
      expect(await driver.layoutsMobileDisplayVisible()).toBe(true);

      await driver.profileMobileSwitchTo("kanban");
      expect(await driver.profileMobileKanbanVisible()).toBe(true);
      await driver.profileMobileReloadIssues();
      expect(await driver.profileMobileActiveLayout()).toBe("kanban");

      await driver.profileMobileSwitchTo("list");
      expect(await driver.layoutsListVisible()).toBe(true);
      await driver.profileMobileReloadIssues();
      expect(await driver.profileMobileActiveLayout()).toBe("list");
    }
  );

  test(
    specTitle(ROW_008, "narrow sidebar starts collapsed, toggles over content, and collapses again"),
    { tag: specTags(ROW_008) },
    async ({ driver }) => {
      test.setTimeout(720_000);
      const world = await setupProfileWorld("parity-prof008m");
      const member = await seatProfileUser(world, ROLE.MEMBER, "parity-prof008mm", true);
      await createIssue(world.workspaceSlug, world.projectId, world.owner.cookie, `Prof mob side ${world.tag}`, {
        assignee_ids: [member.userId],
      });

      await driver.profileOpen(world.workspaceSlug, member.userId, browserSessionCookies(world.owner));
      expect(await driver.profileSidebarVisible()).toBe(false);

      await driver.profileSidebarToggle();
      expect(await driver.profileSidebarVisible()).toBe(true);
      expect(await driver.profileSidebarProjectRows()).toHaveLength(1);
      expect(await driver.profileOverviewCards()).toHaveLength(3);

      await driver.profileSidebarToggle();
      expect(await driver.profileSidebarVisible()).toBe(false);
    }
  );
});
