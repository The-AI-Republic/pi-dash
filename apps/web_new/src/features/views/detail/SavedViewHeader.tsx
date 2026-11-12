// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view detail header (parity/specs/views.md §7, VIEW-021-024). The
// trail reads project, the Views list, then the open view. The project
// crumb and the Views crumb navigate; the last crumb is the switcher. An
// owner-only record adds a lock beside the trail. Nothing else lives here:
// the layout, Display, filter, Add and overflow controls belong to the
// following slice (VIEW-025-027, 048).

import type { SavedView } from "@pidash/api-client";
import { Button, Tooltip } from "@pidash/kit";
import { ChevronRight, Lock } from "lucide-react";
import * as React from "react";

import { PROJECT_VIEWS_CRUMB, projectViewsListHref } from "../lib/projectViews.js";
import { SavedViewSwitcher } from "./SavedViewSwitcher.js";
import { showsPrivateLock } from "./model.js";

export interface SavedViewHeaderProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  view: SavedView;
  /** Visible views in the list screen's order, for the switcher. */
  views: readonly SavedView[];
  viewsLoading: boolean;
  viewsFailed: boolean;
  onOpenProject: () => void;
  onOpenView: (viewId: string) => void;
}

export function SavedViewHeader({
  workspaceSlug,
  projectId,
  projectName,
  view,
  views,
  viewsLoading,
  viewsFailed,
  onOpenProject,
  onOpenView,
}: SavedViewHeaderProps): React.ReactElement {
  return (
    <header
      data-testid="saved-view-header"
      className="flex shrink-0 items-center gap-(--space-2) border-b border-(--border) px-(--space-4) py-(--space-2)"
    >
      <nav aria-label="Breadcrumb" className="min-w-0 flex-1">
        <ol className="flex min-w-0 items-center gap-(--space-1)">
          <li className="flex min-w-0 items-center">
            <Button variant="ghost" size="small" className="max-w-48" onClick={onOpenProject}>
              <span className="truncate">{projectName}</span>
            </Button>
          </li>
          <li className="flex min-w-0 items-center">
            <ChevronRight size={14} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
            <a
              href={projectViewsListHref(workspaceSlug, projectId)}
              className="text-body rounded-(--radius-control) px-(--space-1) text-(--text-muted) hover:text-(--text)"
            >
              {PROJECT_VIEWS_CRUMB}
            </a>
          </li>
          <li className="flex min-w-0 items-center">
            <ChevronRight size={14} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
            <SavedViewSwitcher
              current={view}
              views={views}
              loading={viewsLoading}
              failed={viewsFailed}
              onPick={onOpenView}
            />
          </li>
        </ol>
      </nav>
      {showsPrivateLock(view) ? (
        <Tooltip content="Private">
          <span
            data-testid="saved-view-lock"
            aria-label="Private"
            className="flex shrink-0 cursor-default items-center text-(--text-muted)"
          >
            <Lock size={14} strokeWidth={1.5} aria-hidden="true" />
          </span>
        </Tooltip>
      ) : null}
    </header>
  );
}
