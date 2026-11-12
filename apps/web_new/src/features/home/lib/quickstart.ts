// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quickstart checklist helpers (SHELL-008, SHELL-009, SHELL-010; written
// from parity/specs/shell.md §2 and the quickstart oracle). Four cards
// onboard fresh workspaces: each carries an oracle-pinned title and action
// label, a role gate, and its own completion rule. Dismissal and settings
// and profile visits persist per workspace in local storage.

export type QuickstartCardId = "create-project" | "invite-team" | "configure-workspace" | "personalize-account";

/** Card render order across the grid. */
export const QUICKSTART_CARD_IDS: readonly QuickstartCardId[] = [
  "create-project",
  "invite-team",
  "configure-workspace",
  "personalize-account",
];

// Card titles and action labels are pinned by the parity suite (the specs
// match them by name), so they repeat the oracle contract verbatim. The
// descriptions below are fresh copy for the new app.
export const QUICKSTART_TITLES: Record<QuickstartCardId, string> = {
  "create-project": "Create a project",
  "invite-team": "Invite your team",
  "configure-workspace": "Set up your workspace",
  "personalize-account": "Make Pi Dash yours",
};

export const QUICKSTART_ACTIONS: Record<QuickstartCardId, string> = {
  "create-project": "Get started",
  "invite-team": "Get them in",
  "configure-workspace": "Configure this workspace",
  "personalize-account": "Personalize now",
};

export const QUICKSTART_DESCRIPTIONS: Record<QuickstartCardId, string> = {
  "create-project": "Most things start with a project: create one to begin tracking work.",
  "invite-team": "Bring coworkers in so the team can build and ship together.",
  "configure-workspace": "Tune features and settings to fit how the team works.",
  "personalize-account": "Set your picture, name and preferences across Pi Dash.",
};

/** Cards whose completion is a recorded visit rather than server state. */
export type QuickstartVisitFlag = "visited_workspace" | "visited_profile";

/** Per-workspace quickstart record: dismissal plus the two visit flags. */
export interface QuickstartFlags {
  hide: boolean;
  visited_workspace: boolean;
  visited_profile: boolean;
}

export const DEFAULT_QUICKSTART_FLAGS: QuickstartFlags = {
  hide: false,
  visited_workspace: false,
  visited_profile: false,
};

/** The storage record key for one workspace's quickstart flags. */
export function quickstartStorageKey(workspaceSlug: string): string {
  return `web_new.home.quickstart.${workspaceSlug}`;
}

type FlagStorage = Pick<Storage, "getItem" | "setItem">;

/**
 * The usable flag store, or null outside the browser and in storageless
 * modes. The panel keeps working on in-memory flags when this is null.
 */
export function resolveQuickstartStorage(): FlagStorage | null {
  if (typeof globalThis.window === "undefined") return null;
  try {
    return globalThis.window.localStorage;
  } catch {
    return null;
  }
}

/**
 * Read one workspace's flags, tolerating a missing store, missing record
 * and corrupt JSON: every failure degrades to the default flags.
 */
export function readQuickstartFlags(storage: FlagStorage | null | undefined, workspaceSlug: string): QuickstartFlags {
  if (storage === null || storage === undefined) return { ...DEFAULT_QUICKSTART_FLAGS };
  let raw: string | null;
  try {
    raw = storage.getItem(quickstartStorageKey(workspaceSlug));
  } catch {
    return { ...DEFAULT_QUICKSTART_FLAGS };
  }
  if (raw === null || raw === "") return { ...DEFAULT_QUICKSTART_FLAGS };
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw) as unknown;
  } catch {
    return { ...DEFAULT_QUICKSTART_FLAGS };
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { ...DEFAULT_QUICKSTART_FLAGS };
  }
  const flags = parsed as Partial<QuickstartFlags>;
  return {
    hide: flags.hide === true,
    visited_workspace: flags.visited_workspace === true,
    visited_profile: flags.visited_profile === true,
  };
}

/** Persist one workspace's flags; storage failures keep memory only. */
export function writeQuickstartFlags(
  storage: FlagStorage | null | undefined,
  workspaceSlug: string,
  flags: QuickstartFlags
): void {
  if (storage === null || storage === undefined) return;
  try {
    storage.setItem(quickstartStorageKey(workspaceSlug), JSON.stringify(flags));
  } catch {
    // Private modes and quota errors: the panel already holds the flags
    // in memory, so there is nothing left to report.
  }
}

/** Cards that navigate (every card except project creation). */
export type QuickstartLinkCardId = Exclude<QuickstartCardId, "create-project">;

/**
 * Where a link card leads: the workspace members and settings pages, and
 * the account settings page inside the current workspace.
 */
export function quickstartLinkHref(cardId: QuickstartLinkCardId, workspaceSlug: string): string {
  switch (cardId) {
    case "invite-team":
      return `/${workspaceSlug}/settings/members`;
    case "configure-workspace":
      return `/${workspaceSlug}/settings`;
    case "personalize-account":
      // Account settings live inside the workspace the card is shown in
      // (main#670), so the card needs the slug the others already use.
      return `/${workspaceSlug}/settings/account/general/`;
  }
}

/**
 * The visit a link card records when followed. The invite card completes
 * off the member count instead, so it records nothing.
 */
export function quickstartVisitFlag(cardId: QuickstartCardId): QuickstartVisitFlag | null {
  switch (cardId) {
    case "configure-workspace":
      return "visited_workspace";
    case "personalize-account":
      return "visited_profile";
    default:
      return null;
  }
}

/** Projects the user joined: list rows carrying a membership role. */
export function countJoinedProjects(projects: ReadonlyArray<{ member_role: number | null }>): number {
  return projects.filter((project) => project.member_role !== null).length;
}

export interface QuickstartCompletionInput {
  joinedProjectCount: number;
  totalMembers: number;
  flags: QuickstartFlags;
}

/**
 * One card's done state: joining a project, reaching two members, or
 * visiting settings or the profile flips that card.
 */
export function isQuickstartCardDone(cardId: QuickstartCardId, input: QuickstartCompletionInput): boolean {
  switch (cardId) {
    case "create-project":
      return input.joinedProjectCount > 0;
    case "invite-team":
      return input.totalMembers >= 2;
    case "configure-workspace":
      return input.flags.visited_workspace;
    case "personalize-account":
      return input.flags.visited_profile;
  }
}

/**
 * Panel visibility: dismissal hides it, and it auto-hides once the
 * workspace is lived-in (joined projects with more than one member).
 * The panel mounts on the home route only, so wiki-style paths never
 * see it by construction.
 */
export function isQuickstartPanelVisible(input: QuickstartCompletionInput): boolean {
  if (input.flags.hide) return false;
  if (input.joinedProjectCount > 0 && input.totalMembers >= 2) return false;
  return true;
}

/**
 * Whether a card's action renders: guests get no project creation,
 * non-admins get no invite or setup links, and the profile action is
 * available to everyone. Forbidden actions render nothing clickable.
 */
export function canShowQuickstartAction(
  cardId: QuickstartCardId,
  permissions: { canEdit: boolean; isAdmin: boolean }
): boolean {
  switch (cardId) {
    case "create-project":
      return permissions.canEdit;
    case "invite-team":
      return permissions.isAdmin;
    case "configure-workspace":
      return permissions.isAdmin;
    case "personalize-account":
      return true;
  }
}
