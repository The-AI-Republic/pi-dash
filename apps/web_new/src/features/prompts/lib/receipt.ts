// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Receipt presentation model (parity/specs/agents.md §7, AGT-028-031).
// The API hands back an assembled template plus the resolved section
// list; this module turns those into the rendered count copy, the
// numbered composition, the automatic-run contrast and the target the
// preview form collects. Pure: no queries or components.

import type { CompiledPrompt, PromptPreviewInput, ResolvedPromptSection } from "@pidash/api-client";

import type { PromptKind } from "./sections.js";

/** One numbered entry in a receipt's composition. */
export interface ReceiptSlot {
  /** Zero-padded position, "01" upward. */
  num: string;
  key: string;
  title: string;
  source: string;
}

/** Singular/plural copy for a section count, e.g. "1 section" / "3 sections". */
export function sectionCountLabel(count: number): string {
  return count === 1 ? "1 section" : `${count} sections`;
}

/**
 * The composition list: the section order the compiled template follows,
 * numbered from one. Rendered, never a placeholder template.
 */
export function receiptSlots(sections: ResolvedPromptSection[]): ReceiptSlot[] {
  return sections.map((section, index) => ({
    num: String(index + 1).padStart(2, "0"),
    key: section.key,
    title: section.title,
    source: section.source,
  }));
}

/** Anchor the side nav and cards agree on. */
export function receiptAnchorId(kind: PromptKind): string {
  return `prompt-receipt-${kind}`;
}

/**
 * The automatic-run template to contrast, or null when the viewer holds
 * no overrides (the API omits the variant) or it repeats the personal one.
 */
export function automaticTemplate(compiled: CompiledPrompt): string | null {
  const automatic = compiled.automatic_template_body;
  if (automatic === undefined || automatic === compiled.template_body) return null;
  return automatic;
}

/** Which id a kind renders against: an issue, or a scheduler install. */
export type PreviewTargetMode = "issue" | "install";

export function previewTargetMode(kind: PromptKind): PreviewTargetMode {
  return kind === "scheduler" ? "install" : "issue";
}

/** Field caption for the target input. */
export function previewTargetLabel(kind: PromptKind): string {
  return previewTargetMode(kind) === "install" ? "Scheduler install id" : "Issue id";
}

/** Placeholder hinting that the target is a raw UUID. */
export function previewTargetPlaceholder(kind: PromptKind): string {
  return `${previewTargetLabel(kind)} (UUID)`;
}

/**
 * The preview payload's target field. Draft previews add the scope,
 * section key and body so the server renders the unsaved edit in place
 * of the stored section.
 */
export function previewTargetPayload(
  kind: PromptKind,
  target: string,
  draft?: { scope: "workspace" | "user"; sectionKey: string; body: string }
): PromptPreviewInput {
  const targetField = previewTargetMode(kind) === "install" ? { binding_id: target } : { issue_id: target };
  if (draft === undefined) return targetField;
  return {
    ...targetField,
    scope: draft.scope,
    section_key: draft.sectionKey,
    body: draft.body,
  };
}

/**
 * Draft previews offer a kind switcher only when the edited section is
 * carried by more than one kind; a single-kind section previews in place.
 */
export function draftPreviewKinds(kinds: PromptKind[]): PromptKind[] {
  return kinds.length > 1 ? [...kinds] : [];
}

/**
 * The kind the draft panel previews on open: the section's own kind when
 * there is exactly one, otherwise the first offered.
 */
export function initialDraftKind(kinds: PromptKind[]): PromptKind {
  return kinds[0] ?? "coding-task";
}
