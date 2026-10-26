// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archive, restore and the archived-modules screen model
// (parity/specs/modules.md §7.3–§7.6, §2.4–§2.7; MOD-026/027/028). Pure:
// the dialog and notice copy, where each outcome lands, the archived
// screen's empty-state decision, the dimensions it offers and the stored
// narrowing it accepts. Nothing here touches React, the router or the
// network.

import { MODULE_STATUSES, type ArchivedModuleListItem, type ModuleRow, type ModuleStatus } from "@pidash/api-client";

import {
  emptyModuleFilters,
  hasModuleFilters,
  searchModules,
  moduleMatchesFilters,
  type ModuleFilterDimension,
  type ModuleListFilters,
} from "./moduleListControls.js";

// ---------------------------------------------------------------- copy

/** Heading of the archive confirmation (§7.4): it names the module. */
export const MODULE_ARCHIVE_HEADING = "Archive module";

export function moduleArchiveDialogTitle(name: string): string {
  return `${MODULE_ARCHIVE_HEADING} “${name}”`;
}

/**
 * Body of the confirmation: it names the module and says the archive can
 * be undone. The oracle looks for the word “restored” (§7.4).
 */
export function moduleArchiveDialogBody(name: string): string {
  return `“${name}” will leave the modules list. Nothing is deleted: it can be restored from this project's archives at any time.`;
}

/** Success notice title the oracle reads (§7.4). */
export const MODULE_ARCHIVE_SUCCESS_TITLE = "Archive success";

/** Success notice body: it points at the project archives (§7.4). */
export const MODULE_ARCHIVE_SUCCESS_BODY = "The module now lives in this project's archives.";

/** Label of the confirmation's submit action. */
export const MODULE_ARCHIVE_CONFIRM = "Archive";

// ---------------------------------------------------------------- landing

/**
 * Where an archive lands the viewer (§7.4): the project's modules list.
 * The peek key is dropped, because the module it named just left the live
 * list.
 */
export function moduleArchiveLanding(
  workspaceSlug: string,
  projectId: string
): {
  to: string;
  search: Record<string, never>;
  replace: true;
} {
  return {
    to: `/${workspaceSlug}/projects/${projectId}/modules/`,
    search: {},
    replace: true,
  };
}

// ------------------------------------------------------------ archived state

/**
 * What the archived screen renders beneath its header. The three empty
 * kinds are distinct on purpose (§7.3): a project with no archives gets
 * onboarding, a search that hides everything gets search guidance, and
 * filters that hide everything get filter guidance with a way to clear.
 */
export type ArchivedModulesBody = "skeleton" | "error" | "zero" | "no-match-search" | "no-match-filters" | "rows";

export interface ArchivedModulesBodyInput {
  /** The archived list read is still in flight. */
  listPending: boolean;
  /** The archived list read failed; a retry is offered instead of an empty. */
  listError?: boolean | undefined;
  /** Every archived module the project has. */
  totalCount: number;
  /** Archived modules left after the name search. */
  searchedCount: number;
  /** Archived modules left after search and filters. */
  visibleCount: number;
}

export function resolveArchivedModulesBody({
  listPending,
  listError = false,
  totalCount,
  searchedCount,
  visibleCount,
}: ArchivedModulesBodyInput): ArchivedModulesBody {
  if (listPending) return "skeleton";
  // A failed read is not proof of an empty archive.
  if (listError) return "error";
  if (totalCount === 0) return "zero";
  if (visibleCount > 0) return "rows";
  // Nothing survives both steps. When the search alone already excluded
  // every row the search carries the blame; otherwise the filters did it.
  // Attribution stays stable whichever control the viewer touched last.
  return searchedCount === 0 ? "no-match-search" : "no-match-filters";
}

/** Filter dimensions the archived header offers — no favorites, no status. */
export const ARCHIVED_MODULE_FILTER_DIMENSIONS: readonly ModuleFilterDimension[] = [
  "lead",
  "members",
  "start-date",
  "due-date",
];

/**
 * The stored narrowing the archived screen may apply. Status and favorites
 * describe live modules only (an archived module is finished), so a value
 * carried in from elsewhere never narrows this screen (§7.3).
 */
export function archivedModuleFilters(filters: ModuleListFilters): ModuleListFilters {
  return {
    ...emptyModuleFilters(),
    lead: [...filters.lead],
    members: [...filters.members],
    "start-date": [...filters["start-date"]],
    "due-date": [...filters["due-date"]],
  };
}

/** Whether the archived narrowing holds anything (chips show, empty changes). */
export function hasArchivedModuleFilters(filters: ModuleListFilters): boolean {
  return hasModuleFilters(archivedModuleFilters(filters));
}

/** Archived rows left by the name search alone. */
export function archivedModulesSearched<T extends Pick<ModuleRow, "name">>(rows: readonly T[], query: string): T[] {
  return searchModules(rows, query);
}

/** Archived rows left by search and the archived narrowing together. */
export function archivedModulesVisible<T extends ModuleRow>(
  rows: readonly T[],
  query: string,
  filters: ModuleListFilters,
  now: Date = new Date()
): T[] {
  return archivedModulesSearched(rows, query).filter((row) =>
    moduleMatchesFilters(row, archivedModuleFilters(filters), now)
  );
}

// ------------------------------------------------------------ row shape

function archivedStatus(status: string): ModuleStatus {
  const found = MODULE_STATUSES.find((candidate) => candidate === status);
  // Archived modules are finished; an unknown value still has to render as
  // a status, so it reads as completed rather than dropping the row.
  return found ?? "completed";
}

/**
 * The archived list row in the shape the shared live row renders. The
 * archived projection omits editor-only columns the row never reads; the
 * fields it does send are copied unchanged.
 */
export function toArchivedModuleRow(row: ArchivedModuleListItem): ModuleRow {
  return {
    id: row.id,
    workspace_id: row.workspace_id,
    project_id: row.project_id,
    name: row.name,
    description: row.description,
    description_text: row.description_text,
    description_html: row.description_html,
    start_date: row.start_date,
    target_date: row.target_date,
    status: archivedStatus(row.status),
    lead_id: row.lead_id,
    member_ids: [...row.member_ids],
    sort_order: row.sort_order,
    is_favorite: row.is_favorite,
    total_issues: row.total_issues,
    completed_issues: row.completed_issues,
    cancelled_issues: row.cancelled_issues,
    started_issues: row.started_issues,
    unstarted_issues: row.unstarted_issues,
    backlog_issues: row.backlog_issues,
    created_at: row.created_at,
    updated_at: row.updated_at,
    archived_at: row.archived_at,
  };
}
