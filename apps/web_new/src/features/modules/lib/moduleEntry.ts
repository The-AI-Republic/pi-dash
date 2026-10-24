// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One contract for every module address (parity/specs/modules.md §10.3,
// §11.3–§11.4, MOD-038, MOD-042, MOD-043). A module is reachable at
// exactly two shapes — the list (`…/modules/`, optionally carrying the
// `peekModule` key) and the detail (`…/modules/{id}`) — and the archived
// peek at `…/archives/modules?peekModule={id}`. Copy link, open in new
// tab, the switcher and the peek all build through these helpers, so a
// copied address always resolves to the state it was copied from and
// there is no alternate URL shape to keep in step.
//
// The route list here is the entry inventory the edition and desktop
// negatives read: which addresses exist, which key each one carries, and
// what each one fetches. Nothing in it names a build or an edition,
// because no module capability is gated by either (MOD-042, MOD-043).

import { moduleDetailHref, modulesListHref } from "./moduleList.js";

/** The search key that opens a module quick look over a list screen. */
export const MODULE_PEEK_PARAM = "peekModule";

/** Proof that a `peekModule` value is a module id and not a free string. */
const MODULE_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function withPeek(base: string, moduleId: string): string {
  return `${base}?${MODULE_PEEK_PARAM}=${encodeURIComponent(moduleId)}`;
}

/** The active list address, with the peek key when one module is open. */
export function modulesPeekHref(workspaceSlug: string, projectId: string, moduleId: string | null): string {
  const base = modulesListHref(workspaceSlug, projectId);
  return moduleId === null ? base : withPeek(base.replace(/\/$/, ""), moduleId);
}

/**
 * The archived-modules address with the peek key. The archived screen
 * shares the key name with the live list; the path is what makes the
 * panel read the archived record instead of the live one.
 */
export function archivedModulePeekHref(workspaceSlug: string, projectId: string, moduleId: string | null): string {
  const base = `/${workspaceSlug}/projects/${projectId}/archives/modules`;
  return moduleId === null ? base : withPeek(base, moduleId);
}

/**
 * The address one copy-link acts on. Live modules take the detail shape;
 * an archived module keeps its archival meaning on the archived screen,
 * where its peek is the only resolvable view.
 */
export function moduleShareHref(args: {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  archived: boolean;
}): string {
  if (args.archived) {
    return archivedModulePeekHref(args.workspaceSlug, args.projectId, args.moduleId);
  }
  return moduleDetailHref(args.workspaceSlug, args.projectId, args.moduleId);
}

/** Read the peek key out of raw router search; a non-id reads as absent. */
export function readPeekModule(raw: Record<string, unknown>): string | null {
  const value = raw[MODULE_PEEK_PARAM];
  if (typeof value !== "string" || !MODULE_ID.test(value)) return null;
  return value;
}

/** How one module screen is reached; the entry inventory below lists them. */
export interface ModuleEntry {
  /** Stable name the tests address the entry by. */
  id: "list" | "peek" | "archived-peek" | "detail" | "settings-switch" | "archive";
  /** The address template the entry renders at. */
  path: string;
  /** Whether the entry is a list screen that can carry the peek key. */
  peekKey: boolean;
}

/**
 * Every module address the area can render, in navigation order. The
 * list deliberately has no "desktop" or "edition" flag: these entries are
 * the same in the web build, the desktop shell and every edition, and the
 * negatives' audit reads this list to prove it.
 */
export const MODULE_ENTRIES: readonly ModuleEntry[] = Object.freeze([
  { id: "list", path: "/{ws}/projects/{projectId}/modules/", peekKey: false },
  { id: "peek", path: "/{ws}/projects/{projectId}/modules?peekModule={moduleId}", peekKey: true },
  { id: "detail", path: "/{ws}/projects/{projectId}/modules/{moduleId}", peekKey: false },
  { id: "archive", path: "/{ws}/projects/{projectId}/archives/modules", peekKey: false },
  {
    id: "archived-peek",
    path: "/{ws}/projects/{projectId}/archives/modules?peekModule={moduleId}",
    peekKey: true,
  },
  { id: "settings-switch", path: "/{ws}/settings/projects/{projectId}/features/modules", peekKey: false },
]);

/** The module endpoints an entry may fetch; anything else is off-budget. */
export const MODULE_ENDPOINT_PATTERNS: readonly RegExp[] = Object.freeze([
  /\/modules\/?$/,
  /\/modules\/[0-9a-f-]+\/?$/,
  /archived-modules/,
  /module-links/,
  /user-favorite-modules/,
  /\/modules\/[0-9a-f-]+\/issues/,
]);

/** Whether a request path belongs to a module screen's known traffic. */
export function isModuleEndpointPath(path: string): boolean {
  return MODULE_ENDPOINT_PATTERNS.some((pattern) => pattern.test(path));
}
