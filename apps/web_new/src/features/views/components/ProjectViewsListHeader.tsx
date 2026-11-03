// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project views list header (parity/specs/views.md §3, VIEW-001). Trail
// crumbs, then the control row and the creation button on the right. The
// button renders for every role that reached the list, guests included;
// creation itself is left to the server (VIEW-003).

import { Breadcrumbs, Button } from "@pidash/kit";
import { Plus } from "lucide-react";
import * as React from "react";

import { PROJECT_VIEWS_CRUMB } from "../lib/projectViews.js";

export interface ProjectViewsListHeaderProps {
  workspaceName: string;
  projectName: string;
  /** Opens the create dialog. Slot owner: NEWFRONT-721. */
  onCreateView?: (() => void) | undefined;
  /** Expanding name search. Slot owner: NEWFRONT-720. */
  searchControl?: React.ReactNode;
  /** Sort menu trigger. Slot owner: NEWFRONT-720. */
  sortControl?: React.ReactNode;
  /** Filters trigger. Slot owner: NEWFRONT-720. */
  filterControl?: React.ReactNode;
}

export function ProjectViewsListHeader({
  workspaceName,
  projectName,
  onCreateView,
  searchControl,
  sortControl,
  filterControl,
}: ProjectViewsListHeaderProps): React.ReactElement {
  return (
    <div data-testid="project-views-header" className="flex flex-col gap-(--space-3)">
      <div className="flex items-center justify-between gap-(--space-3)">
        <div className="min-w-0 flex-1">
          <Breadcrumbs entries={[{ label: workspaceName }, { label: projectName }, { label: PROJECT_VIEWS_CRUMB }]} />
        </div>
        <Button variant="primary" size="small" onClick={onCreateView}>
          <Plus size={16} strokeWidth={1.5} aria-hidden="true" />
          Add view
        </Button>
      </div>
      {searchControl === undefined && sortControl === undefined && filterControl === undefined ? null : (
        <div data-testid="project-views-controls" className="flex items-center gap-(--space-2)">
          {searchControl}
          {sortControl}
          {filterControl}
        </div>
      )}
    </div>
  );
}
