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
  projectId: string;
  projectName: string;
  tag: string;
}

/** Fresh owner, workspace and views-enabled project; nothing shared. */
export async function viewsHarness(prefix: string, issueViewsView = true): Promise<ViewsHarness> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const ws = await createWorkspaceForProjects(owner, { name: `Views WS ${tag}`, slug: `views-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  const projectName = `Views Project ${tag}`;
  const projectId = await createProjectViaApi(owner, ws.slug, {
    name: projectName,
    identifier: parityProjectIdentifier("V42"),
  });
  await setProjectViewFlags(ws.slug, projectId, owner.cookie, { issue_views_view: issueViewsView });
  return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, projectId, projectName, tag };
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
