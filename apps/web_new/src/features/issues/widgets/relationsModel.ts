// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Relations model (parity/specs/issues.md §15, ISS-179/181/183). The
// widget shows the four user-facing relation kinds; the server also
// answers timeline kinds that the issue widget never lists. Each kind
// carries its direction wording and tone, and the inverse map is the
// client's record of what the server stores for the other side.

import type { IssueRelationType, IssueRelations, RelatedIssue } from "@pidash/api-client";
import type { LucideIcon } from "lucide-react";
import { CircleDot, CircleSlash, GitCompareArrows, XCircle } from "lucide-react";

/** Kinds the relations widget lists, in menu and group order. */
export const RELATION_KINDS = ["relates_to", "duplicate", "blocked_by", "blocking"] as const;
export type RelationKind = (typeof RELATION_KINDS)[number];

export interface RelationKindSpec {
  kind: RelationKind;
  /** Menu and group title. */
  label: string;
  /** Direction sentence used when the add dialog opens. */
  prompt: string;
  icon: LucideIcon;
  /** Group header tone: blocked danger, blocking warning, the rest neutral. */
  tone: "neutral" | "warning" | "danger";
}

export const RELATION_KIND_SPECS: Record<RelationKind, RelationKindSpec> = {
  relates_to: {
    kind: "relates_to",
    label: "Relates to",
    prompt: "Work items that share context with this one.",
    icon: GitCompareArrows,
    tone: "neutral",
  },
  duplicate: {
    kind: "duplicate",
    label: "Duplicate of",
    prompt: "Work items that describe the same task.",
    icon: CircleSlash,
    tone: "neutral",
  },
  blocked_by: {
    kind: "blocked_by",
    label: "Blocked by",
    prompt: "Work items that must finish before this one.",
    icon: CircleDot,
    tone: "danger",
  },
  blocking: {
    kind: "blocking",
    label: "Blocking",
    prompt: "Work items that wait on this one.",
    icon: XCircle,
    tone: "warning",
  },
};

/**
 * The kind the server keeps on the other issue. The server writes one
 * stored row and derives both directions from it; the map documents
 * which direction a reciprocal row shows.
 */
export const INVERSE_RELATION: Record<RelationKind, RelationKind> = {
  blocked_by: "blocking",
  blocking: "blocked_by",
  relates_to: "relates_to",
  duplicate: "duplicate",
};

/** Rows of one kind, defensively empty when the read omitted it. */
export function relationRows(relations: IssueRelations | undefined, kind: RelationKind): RelatedIssue[] {
  return relations?.[kind] ?? [];
}

/** Visible rows across the four kinds, in widget order. */
export function visibleRelationGroups(
  relations: IssueRelations | undefined
): Array<{ spec: RelationKindSpec; rows: RelatedIssue[] }> {
  return RELATION_KINDS.map((kind) => ({
    spec: RELATION_KIND_SPECS[kind],
    rows: relationRows(relations, kind),
  })).filter((group) => group.rows.length > 0);
}

/** Header count: the four visible kinds only. */
export function relationCount(relations: IssueRelations | undefined): number {
  return RELATION_KINDS.reduce((total, kind) => total + relationRows(relations, kind).length, 0);
}

/** Guard for a menu pick, keeping the surface typed when data is loose. */
export function isRelationKind(value: string): value is RelationKind {
  return (RELATION_KINDS as readonly string[]).includes(value);
}

/** The relation types the widget never renders (timeline edges). */
export const HIDDEN_RELATION_TYPES: readonly IssueRelationType[] = [
  "start_after",
  "start_before",
  "finish_after",
  "finish_before",
];
