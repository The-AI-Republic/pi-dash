// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette server-hit navigation (shell spec, SHELL-084). Maps a grouped
// search hit to the route the app navigates to. Issues and intake items go
// deep through the browse route, projects to their tracking tab, workspaces
// home. Cycle, module, view and page hits land on their project's tracking
// tab (pages carry project_ids, the rest project_id) until their areas land
// their own routes; the owning area deepens its case then. Hits with
// unreadable rows fall back to the workspace home rather than navigating
// nowhere.

import type { SearchResultHit } from "../../../shared/commands/index.js";

export type SearchTarget =
  | { to: "/$ws"; params: { ws: string } }
  | { to: "/$ws/projects/$projectId/issues"; params: { ws: string; projectId: string } }
  | { to: "/$ws/browse/$issueKey"; params: { ws: string; issueKey: string } };

function text(row: Record<string, unknown>, field: string): string | null {
  const value = row[field];
  return typeof value === "string" && value.length > 0 ? value : null;
}

function firstProjectId(row: Record<string, unknown>): string | null {
  const direct = text(row, "project_id");
  if (direct !== null) return direct;
  const list = row["project_ids"];
  if (!Array.isArray(list)) return null;
  const first = list.find((entry): entry is string => typeof entry === "string" && entry.length > 0);
  return first ?? null;
}

function projectFallback(row: Record<string, unknown>, workspaceSlug: string, home: SearchTarget): SearchTarget {
  const projectId = firstProjectId(row);
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
