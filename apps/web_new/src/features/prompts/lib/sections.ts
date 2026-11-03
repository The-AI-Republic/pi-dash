// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompt section presentation model (parity/specs/agents.md §6,
// AGT-023-027, AGT-033). The API resolves sections per kind and scope; the
// browser shows the union of keys across kinds with the viewer's effective
// row, the admin-only workspace baseline beside it, and the edit
// affordances each tier and role allows. Pure: no queries or components.

import type { ResolvedPromptSection } from "@pidash/api-client";

/** Browsed prompt kinds, in page order. */
export const PROMPT_KINDS = ["coding-task", "review", "scheduler"] as const;

export type PromptKind = (typeof PROMPT_KINDS)[number];

/** Kind slug to the card mark the oracle reads (AGT-023). */
export const PROMPT_KIND_LABELS: Record<PromptKind, string> = {
  "coding-task": "Coding task",
  review: "Review",
  scheduler: "Scheduler",
};

/** One kind's resolved list, as the section queries return it. */
export interface PromptKindSections {
  kind: PromptKind;
  sections: ResolvedPromptSection[];
}

/**
 * One browser card: the union entry for a registry key. `user` is the
 * viewer's effective resolution; `workspace` is the admin-only baseline
 * (null for non-admins, who never fetch it).
 */
export interface MergedPromptSection {
  key: string;
  title: string;
  /** Kinds whose recipe carries the key, in page order. */
  kinds: PromptKind[];
  user: ResolvedPromptSection | null;
  workspace: ResolvedPromptSection | null;
}

/** Provenance badge the oracle reads off each card (AGT-023-025). */
export type PromptSourceBadge = "Pi Dash default" | "Workspace override" | "Your override";

/** Governance mark: locked sections, admin-managed sections, or none. */
export type PromptTierMark = "Locked" | "Admin-managed" | null;

/**
 * Union every kind's list into cards, first-seen order. A key keeps the
 * first row seen per scope; overrides are keyed by section, not by kind,
 * so the rung never disagrees across kinds.
 */
export function mergePromptSections(
  userByKind: PromptKindSections[],
  workspaceByKind: PromptKindSections[] | null
): MergedPromptSection[] {
  const byKey = new Map<string, MergedPromptSection>();
  const place = (entry: PromptKindSections, scope: "user" | "workspace"): void => {
    for (const row of entry.sections) {
      let merged = byKey.get(row.key);
      if (merged === undefined) {
        merged = { key: row.key, title: row.title, kinds: [], user: null, workspace: null };
        byKey.set(row.key, merged);
      }
      if (!merged.kinds.includes(entry.kind)) merged.kinds.push(entry.kind);
      if (scope === "user") {
        merged.user ??= row;
      } else {
        merged.workspace ??= row;
      }
    }
  };
  for (const entry of userByKind) place(entry, "user");
  if (workspaceByKind !== null) {
    for (const entry of workspaceByKind) place(entry, "workspace");
  }
  for (const merged of byKey.values()) {
    merged.kinds.sort((left, right) => PROMPT_KINDS.indexOf(left) - PROMPT_KINDS.indexOf(right));
  }
  return [...byKey.values()];
}

/** Map a resolved source rung to its provenance badge. */
export function sourceBadgeFor(source: string): PromptSourceBadge {
  if (source === "workspace") return "Workspace override";
  if (source.startsWith("user:")) return "Your override";
  return "Pi Dash default";
}

/** Map a section tier to the governance mark on its card. */
export function tierMarkFor(customizable: string): PromptTierMark {
  if (customizable === "locked") return "Locked";
  if (customizable === "workspace") return "Admin-managed";
  return null;
}

/** True when the baseline carries a live workspace override. */
export function hasWorkspaceOverride(section: MergedPromptSection): boolean {
  return section.workspace?.source === "workspace";
}

/** True when the viewer keeps a live personal override. */
export function hasPersonalOverride(section: MergedPromptSection): boolean {
  return section.user?.source.startsWith("user:") ?? false;
}

/** Workspace edit button label, or null when the control hides (AGT-024). */
export const CUSTOMIZE_WORKSPACE_LABEL = "Customize for workspace";
/** Workspace edit button label once an override exists (AGT-024). */
export const EDIT_WORKSPACE_LABEL = "Edit workspace default";
/** Personal edit button label, or null when the control hides (AGT-025). */
export const CUSTOMIZE_PERSONAL_LABEL = "Customize for me";
/** Personal edit button label once an override exists (AGT-025). */
export const EDIT_PERSONAL_LABEL = "Edit my override";

export interface PromptViewerGates {
  /** Workspace admin: the only role that edits the baseline. */
  isAdmin: boolean;
  /** Member or admin: guests read without editing. */
  canEdit: boolean;
  /** The baseline lists resolved (never true while they fail: AGT-032). */
  baselineLoaded: boolean;
}

/**
 * The card's edit affordances for a viewer. The tier flags on the
 * effective row decide; a missing effective row offers nothing.
 */
export function editLabelsFor(
  section: MergedPromptSection,
  viewer: PromptViewerGates
): { workspace: string | null; personal: string | null } {
  const row = section.user;
  if (row === null) return { workspace: null, personal: null };
  const workspace =
    viewer.isAdmin && viewer.baselineLoaded && row.editable_at_workspace
      ? hasWorkspaceOverride(section)
        ? EDIT_WORKSPACE_LABEL
        : CUSTOMIZE_WORKSPACE_LABEL
      : null;
  const personal =
    viewer.canEdit && row.editable_at_personal
      ? hasPersonalOverride(section)
        ? EDIT_PERSONAL_LABEL
        : CUSTOMIZE_PERSONAL_LABEL
      : null;
  return { workspace, personal };
}

/** Editor scope: the workspace baseline or the viewer's own override. */
export type PromptEditorScope = "workspace" | "user";

/** Caption above the draft; the oracle matches its scope wording. */
export function editorScopeLabel(scope: PromptEditorScope): string {
  return scope === "workspace" ? "Workspace default" : "Your personal override";
}

/**
 * Seed the draft: the baseline body at workspace scope, the effective
 * body at personal scope.
 */
export function editorSeedFor(section: MergedPromptSection, scope: PromptEditorScope): string {
  if (scope === "workspace") return section.workspace?.body ?? "";
  return section.user?.body ?? "";
}

/** The revert affordance arms only with an override at the edited scope. */
export function revertArmedFor(section: MergedPromptSection, scope: PromptEditorScope): boolean {
  return scope === "workspace" ? hasWorkspaceOverride(section) : hasPersonalOverride(section);
}
