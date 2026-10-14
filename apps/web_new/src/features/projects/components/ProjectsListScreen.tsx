// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Projects-list screen (SHELL-024 through SHELL-027, SHELL-043,
// SHELL-044). Owns the route composition the sibling list slices build
// on: the header, the responsive card grid over the fetched collection,
// the skeleton window while it resolves, and the empty-state decision.
// Sibling seams, all optional: `projects` + `searchQuery` +
// `hasActiveFilters` (NEWFRONT-282 display filtering), `renderCard`
// (NEWFRONT-283, defaults to the card shell), `renderCreateDialog`
// (NEWFRONT-284, mounted when the creation buttons open it), and the
// header control slots passed straight through.
import { Skeleton } from "@pidash/kit";
import * as React from "react";
import type { ProjectDetails } from "@pidash/api-client";

import { ProjectCard } from "./ProjectCard.js";
import { ProjectsEmptyState } from "./ProjectsEmptyState.js";
import { ProjectsListHeader } from "./ProjectsListHeader.js";

export interface CreateDialogControls {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export interface ProjectsListScreenProps {
  workspaceSlug: string;
  /** Archive view: archived rows, the Archived crumb, no creation. */
  archived?: boolean;
  /** Fetched collection, undefined while resolving. */
  collection: ProjectDetails[] | undefined;
  /** Member or admin. Guests get no header button and a disabled action. */
  canCreate: boolean;
  /** Live search text. Slot owner: NEWFRONT-282. */
  searchQuery?: string;
  /** Rows after display filtering. Slot owner: NEWFRONT-282. */
  projects?: ProjectDetails[];
  /** Whether non-search filters narrow the rows. Slot owner: NEWFRONT-282. */
  hasActiveFilters?: boolean;
  /** Expanding list search. Slot owner: NEWFRONT-282. */
  searchControl?: React.ReactNode;
  /** Sort menu trigger. Slot owner: NEWFRONT-282. */
  sortControl?: React.ReactNode;
  /** Filter trigger. Slot owner: NEWFRONT-282. */
  filterControl?: React.ReactNode;
  /** Applied-filter pills strip. Slot owner: NEWFRONT-282. */
  filterPills?: React.ReactNode;
  /** Card renderer. Defaults to the card shell NEWFRONT-283 extends. */
  renderCard?: (project: ProjectDetails) => React.ReactNode;
  /** Creation dialog. Slot owner: NEWFRONT-284. */
  renderCreateDialog?: (controls: CreateDialogControls) => React.ReactNode;
}

export type ProjectsListBody = "loading" | "grid" | "first-run" | "no-match" | "empty-archive";

/** Split the fetched collection into the current view's rows. */
export function selectViewRows(collection: ProjectDetails[], archived: boolean): ProjectDetails[] {
  return collection.filter((project) => (archived ? project.archived_at !== null : project.archived_at === null));
}

export function selectListBody(args: {
  collection: ProjectDetails[] | undefined;
  rowCount: number;
  searchQuery: string;
  hasActiveFilters: boolean;
  archived: boolean;
}): ProjectsListBody {
  if (args.collection === undefined) return "loading";
  if (args.rowCount > 0) return "grid";
  if (args.searchQuery.trim() !== "" || args.hasActiveFilters) return "no-match";
  if (args.archived) return "empty-archive";
  return "first-run";
}

const SKELETON_CARDS = 6;

export function ProjectsListScreen({
  workspaceSlug,
  archived = false,
  collection,
  canCreate,
  searchQuery = "",
  projects,
  hasActiveFilters = false,
  searchControl,
  sortControl,
  filterControl,
  filterPills,
  renderCard,
  renderCreateDialog,
}: ProjectsListScreenProps): React.ReactElement {
  const [createOpen, setCreateOpen] = React.useState(false);
  const rows = projects ?? (collection === undefined ? [] : selectViewRows(collection, archived));
  const body = selectListBody({
    collection,
    rowCount: rows.length,
    searchQuery,
    hasActiveFilters,
    archived,
  });
  const openCreate = React.useCallback(() => setCreateOpen(true), []);
  const renderRow =
    renderCard ?? ((project: ProjectDetails) => <ProjectCard workspaceSlug={workspaceSlug} project={project} />);

  return (
    <section
      aria-label={archived ? "Archived projects" : "Projects"}
      className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
    >
      <ProjectsListHeader
        archived={archived}
        canCreate={canCreate}
        onCreate={openCreate}
        searchControl={searchControl}
        sortControl={sortControl}
        filterControl={filterControl}
      />
      {filterPills}
      {body === "loading" ? (
        <div data-testid="projects-skeleton" className="grid grid-cols-1 gap-(--space-4) md:grid-cols-2 xl:grid-cols-3">
          {Array.from({ length: SKELETON_CARDS }, (_, index) => (
            <div
              key={index}
              className="flex flex-col gap-(--space-2) rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)"
            >
              <Skeleton height="20px" width="60%" />
              <Skeleton height="12px" width="40%" />
              <Skeleton height="12px" />
            </div>
          ))}
        </div>
      ) : null}
      {body === "grid" ? (
        <div data-testid="projects-grid" className="grid grid-cols-1 gap-(--space-4) md:grid-cols-2 xl:grid-cols-3">
          {rows.map((project) => (
            <React.Fragment key={project.id}>{renderRow(project)}</React.Fragment>
          ))}
        </div>
      ) : null}
      {body === "first-run" || body === "no-match" || body === "empty-archive" ? (
        <ProjectsEmptyState variant={body} canCreate={canCreate} onCreate={openCreate} />
      ) : null}
      {renderCreateDialog?.({ open: createOpen, onOpenChange: setCreateOpen })}
    </section>
  );
}
