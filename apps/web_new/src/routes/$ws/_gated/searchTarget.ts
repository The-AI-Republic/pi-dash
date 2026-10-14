// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette server-hit navigation (shell spec, SHELL-084). Maps a grouped
// search hit to the route the app navigates to. Entities whose areas have
// landed go deep — issues through the browse route, projects to their
// tracking tab, workspaces home. Entities without new-app routes yet (the
// cycle, module, view, page and intake areas are Backlog epics) land on the
// closest existing surface, the project tracking tab; the owning area
// deepens its case when its routes land. Hits with unreadable rows fall
// back to the workspace home rather than navigating nowhere.

import type { SearchResultHit } from "../../../shared/commands/index.js";

export type SearchTarget =
  | { to: "/$ws"; params: { ws: string } }
  | { to: "/$ws/projects/$projectId/issues"; params: { ws: string; projectId: string } }
  | { to: "/$ws/browse/$issueKey"; params: { ws: string; issueKey: string } };

function text(row: Record<string, unknown>, field: string): string | null {
  const value = row[field];
  return typeof value === "string" && value.length > 0 ? value : null;
}

function projectFallback(row: Record<string, unknown>, workspaceSlug: string, home: SearchTarget): SearchTarget {
  const projectId = text(row, "project_id");
  if (projectId !== null) {
    return { to: "/$ws/projects/$projectId/issues", params: { ws: workspaceSlug, projectId } };
  }
  return home;
}

export function resolveSearchTarget(hit: SearchResultHit, workspaceSlug: string): SearchTarget {
  const home: SearchTarget = { to: "/$ws", params: { ws: workspaceSlug } };
  const row = (hit.raw ?? {}) as Record<string, unknown>;
  switch (hit.entity) {
    case "issue":
    case "intake": {
      const identifier = text(row, "project__identifier");
      const sequence = row["sequence_id"];
      if (identifier !== null && typeof sequence === "number") {
        return {
          to: "/$ws/browse/$issueKey",
          params: { ws: workspaceSlug, issueKey: `${identifier}-${sequence}` },
        };
      }
      return projectFallback(row, workspaceSlug, home);
    }
    case "project": {
      const id = text(row, "id");
      if (id !== null) {
        return { to: "/$ws/projects/$projectId/issues", params: { ws: workspaceSlug, projectId: id } };
      }
      return home;
    }
    case "workspace": {
      return { to: "/$ws", params: { ws: text(row, "slug") ?? workspaceSlug } };
    }
    case "cycle":
    case "module":
    case "issue_view":
    case "page":
      return projectFallback(row, workspaceSlug, home);
  }
}
