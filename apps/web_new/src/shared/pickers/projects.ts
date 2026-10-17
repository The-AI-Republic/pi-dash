// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project option shaping (parity/specs/issues.md §17, ISS-211). Joined
// workspace projects in server order with selected-first sorting; the
// current project stays listed unless the caller excludes it. Search
// matches names and short codes.
import type { ProjectList } from "@pidash/api-client";

/** One project row in display order. */
export interface ProjectOption {
  id: string;
  name: string;
  identifier: string;
  logoProps: Record<string, unknown>;
  searchText: string;
  selected: boolean;
}

export interface ProjectOptionsInput {
  projects: ProjectList;
  excludeIds?: readonly string[] | undefined;
  selectedIds: readonly string[];
}

export function selectProjectOptions(input: ProjectOptionsInput): ProjectOption[] {
  const options = input.projects
    .filter((project) => !(input.excludeIds?.includes(project.id) ?? false))
    .map((project) => ({
      id: project.id,
      name: project.name,
      identifier: project.identifier,
      logoProps: project.logo_props,
      searchText: `${project.name} ${project.identifier}`,
      selected: input.selectedIds.includes(project.id),
    }));
  options.sort((left, right) => {
    if (left.selected && !right.selected) return -1;
    if (right.selected && !left.selected) return 1;
    return 0;
  });
  return options;
}

/**
 * The logo tile's mark: the picked emoji glyph, else the short code's
 * initial. Mirrors the projects feature's logo reading (it owns
 * ProjectLogo; shared/ cannot import features/) without sharing code.
 */
export function selectProjectMark(logoProps: Record<string, unknown>, identifier: string): string {
  if (logoProps["in_use"] === "emoji") {
    const emoji = logoProps["emoji"];
    if (typeof emoji === "object" && emoji !== null) {
      const value = (emoji as Record<string, unknown>)["value"];
      if (typeof value === "string" && value.trim() !== "") return value;
    }
  }
  const initial = identifier.trim().slice(0, 1).toUpperCase();
  return initial === "" ? "?" : initial;
}
