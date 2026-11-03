// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views palette commands (parity/specs/views.md §12, VIEW-042). Pure
// builders: the entries component registers exactly what these return,
// so unauthorized entries stay absent instead of disabled (SHELL-092).
// Creation needs the project context, the views switch on, and project
// member-or-better standing; the jump entry only needs the context and
// the switch, because it only navigates. Sequences fire through the
// shell registry; nothing here handles keys.

import type { PidashCommand } from "../../../shared/commands/index.js";

/** Palette command opening the views-owned create dialog. */
export const VIEW_CREATE_COMMAND_ID = "views.create-view";
/** Palette command opening the project-views jump sub-page. */
export const VIEW_JUMP_COMMAND_ID = "views.open-project-view";
/** Sub-page id behind the jump command. */
export const VIEW_JUMP_PAGE_ID = "views.project-view-picker";

/** Creation title the oracle matches (VIEW-042). */
export const VIEW_CREATE_TITLE = "New view";
/** Jump title the oracle matches (VIEW-042). */
export const VIEW_JUMP_TITLE = "Open a project view";

/** Two-key sequence opening the create dialog (VIEW-042). */
export const VIEW_CREATE_SEQUENCE = "nv";
/** Two-key sequence opening the jump menu (VIEW-042). */
export const VIEW_JUMP_SEQUENCE = "ov";

export interface ViewCreateCommandContext {
  /** Project name shown as the entry hint. */
  projectName: string;
  /** The project's `issue_views_view` switch. */
  viewsEnabled: boolean;
  /** Project admin or member standing; guests read false. */
  canCreateView: boolean;
  /** Opens the views-owned create dialog for the project. */
  onCreateView: () => void;
}

export interface ViewJumpCommandContext {
  /** Project name shown as the entry hint. */
  projectName: string;
  /** The project's `issue_views_view` switch. */
  viewsEnabled: boolean;
}

/**
 * The creation entry, or null when the caller may not use it: the
 * switch off, or a guest. Hidden in every other context (VIEW-042).
 */
export function buildViewCreateCommand(context: ViewCreateCommandContext): PidashCommand | null {
  if (!context.viewsEnabled || !context.canCreateView) return null;
  return {
    id: VIEW_CREATE_COMMAND_ID,
    title: VIEW_CREATE_TITLE,
    hint: context.projectName,
    section: "Create",
    sequence: VIEW_CREATE_SEQUENCE,
    keywords: ["create", "add", "new", "view"],
    run: context.onCreateView,
  };
}

/**
 * The jump entry, or null while the switch is off. Every role sees it
 * when the switch is on, guests included: it only navigates (VIEW-042).
 */
export function buildViewJumpCommand(context: ViewJumpCommandContext): PidashCommand | null {
  if (!context.viewsEnabled) return null;
  return {
    id: VIEW_JUMP_COMMAND_ID,
    title: VIEW_JUMP_TITLE,
    hint: context.projectName,
    section: "Navigate",
    sequence: VIEW_JUMP_SEQUENCE,
    page: VIEW_JUMP_PAGE_ID,
    run: () => undefined,
  };
}
