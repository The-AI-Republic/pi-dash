// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages palette wiring (parity/specs/pages.md §13.6, PAGE-090). The list
// screen registers one navigation command that steps into the palette's
// page picker; the picker itself reads the whole visible forest and lists
// child pages with their parent path. No page-owned key binding: the shell
// owns every shortcut.

import type { PidashCommand } from "../../../shared/commands/index.js";

/** Palette command opening the pages jump sub-page. */
export const PAGE_JUMP_COMMAND_ID = "pages.open-page";
/** Sub-page id behind the jump command. */
export const PAGE_JUMP_PAGE_ID = "pages.page-picker";
/** Command title and prompt while the picker is open. */
export const PAGE_JUMP_TITLE = "Open a page";
/** Two-key sequence opening the picker. */
export const PAGE_JUMP_SEQUENCE = "op";

export interface PageJumpCommandContext {
  /** Project name shown as the entry hint. */
  projectName: string;
  /** The project's pages feature switch. */
  pagesEnabled: boolean;
}

/** The jump entry, or null while the pages feature is off. */
export function buildPageJumpCommand(context: PageJumpCommandContext): PidashCommand | null {
  if (!context.pagesEnabled) return null;
  return {
    id: PAGE_JUMP_COMMAND_ID,
    title: PAGE_JUMP_TITLE,
    hint: context.projectName,
    section: "Navigate",
    sequence: PAGE_JUMP_SEQUENCE,
    page: PAGE_JUMP_PAGE_ID,
    run: () => undefined,
  };
}
