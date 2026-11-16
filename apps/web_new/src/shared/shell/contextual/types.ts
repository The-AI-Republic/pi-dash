// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contextual palette shared types (shell spec, SHELL-090/091). Entity
// screens register their contextual entries while mounted; the palette
// groups them under the family headings below and names the subject in
// the context chip. Capability inputs arrive as plain data so the
// selectors stay pure and the owning routes assemble them from their
// feature hooks (shared/ never imports features/).

import type { PaletteCommandContext } from "../../commands/registry.js";

/** Entity families with a contextual palette group. */
export type ContextKind = PaletteCommandContext["kind"];

/** Palette group headings per family. */
export const CONTEXT_SECTIONS: Record<ContextKind, string> = {
  "work-item": "Work item actions",
  cycle: "Cycle actions",
  module: "Module actions",
  page: "Page actions",
};

/** Chip entity labels per family. */
export const CONTEXT_INDICATORS: Record<ContextKind, string> = {
  "work-item": "Work item",
  cycle: "Cycle",
  module: "Module",
  page: "Page",
};

/** Project standing behind the work-item/cycle/module edit gates. */
export interface ProjectStanding {
  /** Project role number (20 admin, 15 member, 5 guest), null when outside. */
  memberRole: number | null;
  /** True when the project is archived (edits hidden). */
  archived: boolean;
}

/** True when the caller may edit: project admin or member, not archived. */
export function canEditInProject(standing: ProjectStanding): boolean {
  return standing.archived === false && (standing.memberRole === 20 || standing.memberRole === 15);
}

/** Work-item kind wording for toasts (epics and sub-items name themselves). */
export function workItemNoun(input: { isEpic: boolean; parentId: string | null }): string {
  if (input.isEpic) return "Epic";
  if (input.parentId !== null) return "Sub-work item";
  return "Work item";
}
