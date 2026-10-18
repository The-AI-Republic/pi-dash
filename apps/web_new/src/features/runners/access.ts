// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners access rule (NEWFRONT-540, RUN-001). Role-gated only: no edition
// or flag. The workspace area admits admins and members; a project area
// admits workspace admins and the project's admins/members. Nothing renders
// until the role records have loaded, so the denial never flashes.

import { ROLE_ADMIN, ROLE_MEMBER } from "../../core/session/permissions.js";

export type RunnersAccess = "loading" | "allowed" | "denied";

/** Workspace scope: admin or member standing. */
export function decideWorkspaceRunnersAccess(args: { role: number | null; isLoading: boolean }): RunnersAccess {
  if (args.isLoading) return "loading";
  if (args.role === null) return "denied";
  return args.role >= ROLE_MEMBER ? "allowed" : "denied";
}

/**
 * Project scope: workspace admins always pass; otherwise the caller needs
 * admin or member standing on that project. Either read still loading keeps
 * the screen blank.
 */
export function decideProjectRunnersAccess(args: {
  workspaceRole: number | null;
  workspaceLoading: boolean;
  projectRoles: Record<string, number> | undefined;
  projectRolesLoading: boolean;
  projectId: string;
}): RunnersAccess {
  if (args.workspaceLoading || args.projectRolesLoading) return "loading";
  if (args.workspaceRole !== null && args.workspaceRole >= ROLE_ADMIN) return "allowed";
  const projectRole = args.projectRoles?.[args.projectId];
  if (projectRole === undefined) return "denied";
  return projectRole >= ROLE_MEMBER ? "allowed" : "denied";
}
