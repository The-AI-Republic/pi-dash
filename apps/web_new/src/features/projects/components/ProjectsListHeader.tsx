// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Projects-list header (SHELL-043, SHELL-044). Breadcrumb trail, the
// control slots, and the responsive creation button. Guests and the
// archive view get no creation button. The sort, filter and search
// controls arrive with NEWFRONT-282 and mount into the slots below:
// the desktop filter row (medium screens and up) and the touch-adapted
// two-pane mobile bar (below medium). Both shells mount the same sort
// and filter nodes with one hidden per breakpoint, as in the old app,
// so drivers always act on the visible instance. Every list crumb
// renders as text.
import { Breadcrumbs, Button } from "@pidash/kit";
import { Plus } from "lucide-react";
import * as React from "react";

export interface ProjectsListHeaderProps {
  archived: boolean;
  /** Member or admin. Guests get no creation button at all. */
  canCreate: boolean;
  onCreate: () => void;
  /** Expanding list search. Slot owner: NEWFRONT-282. */
  searchControl?: React.ReactNode;
  /** Sort menu trigger. Slot owner: NEWFRONT-282. */
  sortControl?: React.ReactNode;
  /** Filter trigger. Slot owner: NEWFRONT-282. */
  filterControl?: React.ReactNode;
}

export function ProjectsListHeader({
  archived,
  canCreate,
  onCreate,
  searchControl,
  sortControl,
  filterControl,
}: ProjectsListHeaderProps): React.ReactElement {
  return (
    <div data-testid="projects-list-header" className="flex flex-col gap-(--space-3)">
      <div className="flex items-center justify-between gap-(--space-3)">
        <div className="min-w-0 flex-1">
          <Breadcrumbs entries={archived ? [{ label: "Projects" }, { label: "Archived" }] : [{ label: "Projects" }]} />
        </div>
        {canCreate && !archived ? (
          <Button variant="primary" size="small" onClick={onCreate}>
            <Plus size={16} strokeWidth={1.5} aria-hidden="true" />
            <span className="hidden sm:inline">Add Project</span>
            <span className="sm:hidden">Project</span>
          </Button>
        ) : null}
      </div>
      <div data-testid="projects-desktop-filters" className="hidden items-center gap-(--space-2) md:flex">
        {searchControl}
        {sortControl}
        {filterControl}
      </div>
      <div data-testid="projects-mobile-bar" className="grid grid-cols-2 gap-(--space-2) md:hidden">
        {sortControl}
        {filterControl}
      </div>
    </div>
  );
}
