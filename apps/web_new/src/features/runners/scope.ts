// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Route bases for the runners area (NEWFRONT-540, RUN-002/003/004). One
// page module serves both scopes: a project id in the route rewrites every
// link base, otherwise the aggregate tree is used. Downstream slices build
// their child paths from these helpers so the two trees can never drift.

export interface RunnersScopeInput {
  workspaceSlug: string;
  /** Absent for the workspace-wide aggregate. */
  projectId?: string | undefined;
}

/** `/ws/runners` or `/ws/projects/<id>/runners`. */
export function runnersBase({ workspaceSlug, projectId }: RunnersScopeInput): string {
  return projectId === undefined ? `/${workspaceSlug}/runners` : `/${workspaceSlug}/projects/${projectId}/runners`;
}

/** A child path below the scope base, e.g. `runs`, `detail/<id>`, `chat/<id>`. */
export function runnersPath(scope: RunnersScopeInput, child?: string): string {
  const base = runnersBase(scope);
  return child === undefined ? base : `${base}/${child}`;
}
