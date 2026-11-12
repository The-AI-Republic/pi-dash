// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project option shaping (parity/specs/issues.md §17, ISS-211). Joined
// workspace projects in server order with selected-first sorting; the
// current project stays listed unless the caller excludes it. Search
// matches names and short codes.
import type { ProjectList } from "@pidash/api-client";

import { projectMark, projectMarkText } from "./projectIcon.js";

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
  /** When set, projects outside this list are never offered. */
  includeIds?: readonly string[] | undefined;
  selectedIds: readonly string[];
}

/** One display row from a caller's own project data (no full row needed). */
export function toProjectOption(
  project: { id: string; name: string; identifier: string },
  logoProps: Record<string, unknown> = {}
): ProjectOption {
  return {
    id: project.id,
    name: project.name,
    identifier: project.identifier,
    logoProps,
    searchText: `${project.name} ${project.identifier}`,
    selected: false,
  };
}

export function selectProjectOptions(input: ProjectOptionsInput): ProjectOption[] {
  const options = input.projects
    .filter((project) => input.includeIds === undefined || input.includeIds.includes(project.id))
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
 * The logo tile's text mark: the picked emoji glyph, else the short code's
 * initial. Icon picks have no text form; callers render them through
 * `ProjectMark`, which maps the stored icon name to its glyph (ISS-091).
 */
export function selectProjectMark(logoProps: Record<string, unknown>, identifier: string): string {
  return projectMarkText(projectMark(logoProps), identifier);
}
