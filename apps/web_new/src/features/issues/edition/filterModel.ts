// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition-only sub-issue filter dimensions (parity/specs/issues.md §20/
// §23, ISS-234): the team-project scoping filter and the work-item-type
// filter with its applied chips. The dimensions live outside the base
// filter object, whose keys the community dropdown renders; the
// edition adds them only when its flags are on.

import type { TeamProjectOption, WorkItemTypeOption } from "./providers.js";

/** Values the two edition dimensions carry. */
export interface EditionFilters {
  teamProject: string[];
  issueType: string[];
}

export function emptyEditionFilters(): EditionFilters {
  return { teamProject: [], issueType: [] };
}

/** Toggle one value of one edition dimension. */
export function toggleEditionFilter(filters: EditionFilters, key: keyof EditionFilters, value: string): EditionFilters {
  const current = filters[key];
  const next = current.includes(value) ? current.filter((entry) => entry !== value) : [...current, value];
  return { ...filters, [key]: next };
}

/** How many edition dimensions carry an applied value. */
export function editionFilterCount(filters: EditionFilters): number {
  return (filters.teamProject.length > 0 ? 1 : 0) + (filters.issueType.length > 0 ? 1 : 0);
}

/** Whether a row belongs to any of the applied team projects. */
export function matchesTeamProject(rowProjectId: string, applied: readonly string[]): boolean {
  return applied.length === 0 || applied.includes(rowProjectId);
}

/**
 * Whether a row carries one of the applied types. Rows answer a null type
 * when nothing is set; a null never matches a chosen type.
 */
export function matchesIssueType(rowTypeId: string | null, applied: readonly string[]): boolean {
  return applied.length === 0 || (rowTypeId !== null && applied.includes(rowTypeId));
}

/** One applied chip: its dimension, value and human label. */
export interface EditionChip {
  key: keyof EditionFilters;
  value: string;
  label: string;
}

/** Applied chips in dimension order, each named by its option list. */
export function appliedChips(
  filters: EditionFilters,
  options: { teamProject: readonly TeamProjectOption[]; issueType: readonly WorkItemTypeOption[] }
): EditionChip[] {
  const chips: EditionChip[] = [];
  for (const id of filters.teamProject) {
    const row = options.teamProject.find((option) => option.id === id);
    chips.push({ key: "teamProject", value: id, label: row?.name ?? id });
  }
  for (const id of filters.issueType) {
    const row = options.issueType.find((option) => option.id === id);
    chips.push({ key: "issueType", value: id, label: row?.name ?? id });
  }
  return chips;
}
