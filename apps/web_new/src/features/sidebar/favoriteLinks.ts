// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Favorite entry links (parity/specs/shell.md §4, SHELL-057). Pure:
// the entity-type address table plus the icon kind per row. Known
// types land on their target; anything unrecognized (or missing its
// identifiers) falls back to the workspace root without crashing.

import type { Favorite } from "@pidash/api-client";

/** Row glyph per favorite kind; the component maps these to icons. */
export type FavoriteIconKind = "project" | "page" | "view" | "module" | "cycle" | "folder" | "fallback";

/**
 * Glyph kind for one row: folders render the folder glyph however
 * they are typed, known entity types render their own, and anything
 * else renders the fallback glyph instead of crashing (NEWFRONT-144).
 */
export function resolveFavoriteIconKind(row: Pick<Favorite, "entity_type" | "is_folder">): FavoriteIconKind {
  if (row.is_folder) return "folder";
  switch (row.entity_type) {
    case "project":
      return "project";
    case "page":
      return "page";
    case "view":
      return "view";
    case "module":
      return "module";
    case "cycle":
      return "cycle";
    default:
      return "fallback";
  }
}

/**
 * Sidebar address for one favorite entry (SHELL-057). Projects land on
 * tracking; cycles, modules, views and pages land on their detail
 * page. Unknown types and rows missing their identifiers fall back to
 * the workspace root (NEWFRONT-144).
 */
export function resolveFavoriteHref(
  workspaceSlug: string,
  row: Pick<Favorite, "entity_type" | "entity_identifier" | "project_id">
): string {
  const root = `/${workspaceSlug}`;
  if (row.project_id === null) return root;
  switch (row.entity_type) {
    case "project":
      return `${root}/projects/${row.project_id}/issues`;
    case "cycle":
    case "module":
    case "view":
    case "page":
      if (row.entity_identifier === null) return root;
      return `${root}/projects/${row.project_id}/${row.entity_type}s/${row.entity_identifier}`;
    default:
      return root;
  }
}
