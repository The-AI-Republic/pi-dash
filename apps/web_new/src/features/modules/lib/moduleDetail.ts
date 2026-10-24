// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail screen model (parity/specs/modules.md §8.1, §10.3,
// MOD-029, MOD-038). Pure: the tab title, the switcher pick target, the
// one read-outcome decision the detail gate renders, and the pinned
// work-item badge tooltip the detail frame renders.

import { isApiError } from "@pidash/api-client";

import { decideProjectDenial, type ProjectAccessWallKind } from "../../auth/index.js";
import { moduleDetailHref } from "./moduleList.js";

/** Browser tab title: the project name followed by the module name. */
export function moduleDetailTitle(projectName: string, moduleName: string): string {
  return `${projectName} - ${moduleName}`;
}

/** Project home for a reader: the project's issues screen. */
export function projectIssuesHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/issues`;
}

/**
 * Detail address a switcher pick navigates to. Null stays put —
 * clearing is impossible (the menu hides its clear row) and
 * re-picking the current module is a no-op.
 */
export function switchModuleTarget(
  workspaceSlug: string,
  projectId: string,
  currentModuleId: string,
  pickedModuleId: string | null
): string | null {
  if (pickedModuleId === null || pickedModuleId === currentModuleId) return null;
  return moduleDetailHref(workspaceSlug, projectId, pickedModuleId);
}

/** Gone-away: an archived record, or a 404 on the live detail read. */
export function isModuleGoneAway(error: unknown, archivedAt: string | null | undefined): boolean {
  if (archivedAt !== null && archivedAt !== undefined) return true;
  return isApiError(error) && error.status === 404;
}

/**
 * What the detail gate renders for one read (MOD-038). Three outcomes are
 * distinct and must never collapse into each other:
 *
 * - `content` — the read resolved to a live module;
 * - `missing` — the read says this id does not resolve (404, or an
 *   archived record served on the live address), so the missing-module
 *   state with its way back to the list is the honest answer;
 * - `wall` — the server refused the reader (403, 409), which is about the
 *   project, not the id: the shared project access wall stands in, and
 *   the same id still resolves for a member. Claiming "missing" here
 *   would tell a non-member that a module was deleted.
 *
 * Anything else (a pending read, a transport failure, a 5xx) stays
 * `content`: the route's own error and pending surfaces own it, and the
 * gate must not turn a transient failure into a deletion notice.
 */
export type ModuleReadOutcome = { kind: "content" } | { kind: "missing" } | { kind: "wall"; wall: ModuleWall };

/** The two walls a refusal below the project shell can stand on. */
export type ModuleWall = Exclude<ProjectAccessWallKind, "not-found">;

/** Whether a status is a project-access refusal rather than an id miss. */
export function isAccessRefusal(status: number | undefined): boolean {
  return status === 403 || status === 409;
}

/**
 * The one split between "you cannot open this project", "this module is
 * gone" and "show the module". `archivedAt` is the resolved record's own
 * marker: a live-address read that returns an archived module is a miss.
 * The wall kind comes from the auth area's decision ladder so a module
 * page and the project shell refuse a reader the same way.
 */
export function moduleReadOutcome(args: {
  error: unknown;
  archivedAt: string | null | undefined;
  /** Workspace-admin standing; admins may join a private project. */
  isAdmin: boolean;
}): ModuleReadOutcome {
  if (args.archivedAt !== null && args.archivedAt !== undefined) return { kind: "missing" };
  if (!isApiError(args.error)) return { kind: "content" };
  if (isModuleGoneAway(args.error, null)) return { kind: "missing" };
  if (!isAccessRefusal(args.error.status)) return { kind: "content" };
  // A refusal is about the project, never about the id: the ladder lands
  // on the join or no-access wall. Not-found is what a 404 produces, and
  // that is `missing` above.
  const wall: ModuleWall =
    decideProjectDenial({ status: args.error.status, isAdmin: args.isAdmin }) === "join" ? "join" : "no-access";
  return { kind: "wall", wall };
}

/**
 * Pinned badge tooltip. The singular keeps the plural verb with the
 * singular noun, matching the oracle's pinned shape.
 */
export function moduleBadgeTooltip(total: number): string {
  if (total === 1) return "There are 1 work item in this module";
  return `There are ${total} work items in this module`;
}
