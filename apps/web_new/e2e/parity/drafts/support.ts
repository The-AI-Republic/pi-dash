// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared setup for the drafts oracle specs (NEWFRONT-32, DRAFT-001–027).
// Every scenario mints a fresh owner, workspace and project, so reruns on
// one seeded stack stay green and sibling runs never share drafts.
import type { ParityDriver } from "../drivers/parity-driver";
import {
  addProjectMembersViaApi,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  parityProjectIdentifier,
  seatFreshMember,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";

export interface DraftsHarness {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  workspaceName: string;
  projectId: string;
  projectName: string;
  projectIdentifier: string;
  tag: string;
}

/** Fresh owner, workspace and project; nothing shared. */
export async function draftsHarness(prefix: string): Promise<DraftsHarness> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const workspaceName = `Drafts WS ${tag}`;
  const ws = await createWorkspaceForProjects(owner, { name: workspaceName, slug: `drafts-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  const projectName = `Drafts Project ${tag}`;
  // Identifiers collide across harnesses (two-char salt, shared scope):
  // retry with a fresh identifier instead of failing the scenario.
  let projectId = "";
  let projectIdentifier = "";
  for (let attempt = 0; attempt < 4; attempt += 1) {
    projectIdentifier = parityProjectIdentifier("D32");
    try {
      projectId = await createProjectViaApi(owner, ws.slug, { name: projectName, identifier: projectIdentifier });
      break;
    } catch (error) {
      if (attempt === 3 || !(error instanceof Error) || !/HTTP 400/.test(error.message)) throw error;
    }
  }
  return {
    owner,
    workspaceSlug: ws.slug,
    workspaceId: ws.id,
    workspaceName,
    projectId,
    projectName,
    projectIdentifier,
    tag,
  };
}

/** Seat a fresh workspace user, optionally adding them to the harness project at `projectRole`. */
export async function draftsSeat(
  harness: DraftsHarness,
  role: number,
  prefix: string,
  projectRole?: number
): Promise<AuthedSession> {
  const member = await seatFreshMember(harness.owner, harness.workspaceSlug, role, prefix);
  await markOnboardedForProjects(member);
  await setLastWorkspaceForProjects(member, harness.workspaceId);
  if (projectRole !== undefined) {
    await addProjectMembersViaApi(harness.owner, harness.workspaceSlug, harness.projectId, [
      { member_id: member.userId, role: projectRole },
    ]);
  }
  return member;
}

/** Which settled shape the drafts screen is in (list, empty, or the no-project fallback). */
export async function draftsSettledShape(driver: ParityDriver): Promise<"list" | "empty" | "no-project" | null> {
  if ((await driver.draftBlockCount()) > 0) return "list";
  // A count chip with no rows settles the fetch too: drafts the screen
  // cannot render (project-less, or in an unjoined project) still count
  // toward the server total while their blocks render nothing.
  if ((await driver.draftsCountChip()) !== null) return "list";
  if (await driver.pageTextContains("Half-written work items")) return "empty";
  if (await driver.pageTextContains("No project")) return "no-project";
  return null;
}

/** Open the harness workspace's drafts screen as `user` (pre-authenticated) and wait until it settles. */
export async function draftsOpenAs(
  driver: ParityDriver,
  harness: DraftsHarness,
  user: AuthedSession = harness.owner
): Promise<void> {
  await driver.openAuthenticated(`/${harness.workspaceSlug}/drafts`, browserSessionCookies(user));
  // The drafts fetch can be rate-limited on a busy host; re-entering the
  // route remounts the view and retries it.
  const started = Date.now();
  let revisits = 0;
  for (;;) {
    if ((await draftsSettledShape(driver)) !== null) return;
    if (Date.now() - started > 150_000) throw new Error("[parity] drafts page never settled");
    if (Date.now() - started > 30_000 * (revisits + 1) && revisits < 4) {
      revisits += 1;
      await driver.openAuthenticated(`/${harness.workspaceSlug}/drafts`, browserSessionCookies(user));
    }
    await new Promise((resolve) => setTimeout(resolve, 2000));
  }
}
