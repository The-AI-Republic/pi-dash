// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared views-area harness (NEWFRONT-42): a fresh owner plus workspace
// plus project per spec file, with the views feature enabled unless asked
// otherwise — so list/detail specs never touch the seeded project or each
// other's rows, and role matrices seat real members and guests.
import type { ParityDriver } from "../drivers/index";
import {
  addProjectMembersViaApi,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  parityProjectIdentifier,
  seatFreshMember,
  setLastWorkspaceForProjects,
  setProjectViewFlags,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";

export interface ViewsHarness {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  workspaceName: string;
  projectId: string;
  projectName: string;
  tag: string;
}

/** Fresh owner, workspace and views-enabled project; nothing shared. */
export async function viewsHarness(prefix: string, issueViewsView = true): Promise<ViewsHarness> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const workspaceName = `Views WS ${tag}`;
  const ws = await createWorkspaceForProjects(owner, { name: workspaceName, slug: `views-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  const projectName = `Views Project ${tag}`;
  // Identifiers collide across harnesses (two-char salt, shared scope):
  // retry with a fresh identifier instead of failing the scenario.
  let projectId = "";
  for (let attempt = 0; attempt < 4; attempt += 1) {
    try {
      projectId = await createProjectViaApi(owner, ws.slug, {
        name: projectName,
        identifier: parityProjectIdentifier("V42"),
      });
      break;
    } catch (error) {
      if (attempt === 3 || !(error instanceof Error) || !/HTTP 400/.test(error.message)) throw error;
    }
  }
  await setProjectViewFlags(ws.slug, projectId, owner.cookie, { issue_views_view: issueViewsView });
  return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, workspaceName, projectId, projectName, tag };
}

/** Seat a fresh workspace user and add them to the harness project at `role`. */
export async function viewsSeat(harness: ViewsHarness, role: number, prefix: string): Promise<AuthedSession> {
  const member = await seatFreshMember(harness.owner, harness.workspaceSlug, role, prefix);
  await markOnboardedForProjects(member);
  await setLastWorkspaceForProjects(member, harness.workspaceId);
  await addProjectMembersViaApi(harness.owner, harness.workspaceSlug, harness.projectId, [
    { member_id: member.userId, role },
  ]);
  return member;
}

/** Open the harness project's views list as `user` (pre-authenticated). */
export async function viewsOpenListAs(
  driver: ParityDriver,
  harness: ViewsHarness,
  user: AuthedSession = harness.owner
): Promise<void> {
  await driver.openAuthenticated(
    `/${harness.workspaceSlug}/projects/${harness.projectId}/views`,
    browserSessionCookies(user)
  );
}

/** Open one saved view's detail page as `user` (pre-authenticated). */
export async function viewsOpenDetailAs(
  driver: ParityDriver,
  harness: ViewsHarness,
  viewId: string,
  user: AuthedSession = harness.owner
): Promise<void> {
  await driver.openAuthenticated(
    `/${harness.workspaceSlug}/projects/${harness.projectId}/views/${viewId}`,
    browserSessionCookies(user)
  );
}

/**
 * The display payload the workspace create form writes for a fresh view
 * (observed server state of a UI-created view): spreadsheet layout with
 * the full property set. API-seeded customs need at least the layout —
 * any other layout renders the empty OSS cloud-layout stub.
 */
export function wsSpreadsheetDisplay(): Record<string, unknown> {
  return {
    display_filters: {
      layout: "spreadsheet",
      group_by: null,
      sub_group_by: null,
      order_by: "-created_at",
      show_empty_groups: true,
      show_sub_issues: true,
    },
    display_properties: {
      key: true,
      assignee: true,
      start_date: true,
      due_date: true,
      labels: true,
      priority: true,
      state: true,
      sub_issue_count: true,
      attachment_count: true,
      link: true,
      estimate: true,
    },
    rich_filters: {},
  };
}

/** Open the harness project's views feature settings as `user`. */
export async function viewsSettingsOpenFeaturesViews(
  driver: ParityDriver,
  harness: ViewsHarness,
  user: AuthedSession = harness.owner
): Promise<void> {
  await driver.openAuthenticated(
    `/${harness.workspaceSlug}/settings/projects/${harness.projectId}/features/views`,
    browserSessionCookies(user)
  );
}

/** Open the harness workspace's views list as `user` (pre-authenticated). */
export async function wsViewsOpenListAs(
  driver: ParityDriver,
  harness: ViewsHarness,
  user: AuthedSession = harness.owner
): Promise<void> {
  await driver.openAuthenticated(`/${harness.workspaceSlug}/workspace-views`, browserSessionCookies(user));
}

/** Open one workspace view's detail page as `user` (pre-authenticated). */
export async function wsViewsOpenDetailAs(
  driver: ParityDriver,
  harness: ViewsHarness,
  globalViewId: string,
  user: AuthedSession = harness.owner
): Promise<void> {
  await driver.openAuthenticated(
    `/${harness.workspaceSlug}/workspace-views/${globalViewId}`,
    browserSessionCookies(user)
  );
}
