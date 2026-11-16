// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The customs half of the workspace views list (parity/specs/views.md §9,
// VIEW-039). One of three states: the loading skeleton while the read is
// in flight, the inline retry when it failed, or one row per visible custom.
// No customs renders nothing — the four defaults above stand alone, with no
// empty illustration or call to action. A non-empty collection the search
// excluded renders a quiet no-match line instead of an empty gap.

import type { SavedView } from "@pidash/api-client";
import { Search } from "lucide-react";
import * as React from "react";

import type { WorkspaceCustomsState } from "../lib/workspaceViews.js";
import { ProjectViewsError } from "./ProjectViewsError.js";
import { ProjectViewsSkeleton } from "./ProjectViewsSkeleton.js";
import { WorkspaceViewRow } from "./WorkspaceViewRow.js";

export interface WorkspaceViewsCustomsProps {
  state: WorkspaceCustomsState;
  /** Customs left after the name search, in display order. */
  views: readonly SavedView[];
  /** True when the unfiltered collection has rows the search excluded. */
  searchHidesRows: boolean;
  workspaceSlug: string;
  onEdit?: ((view: SavedView) => void) | undefined;
  onDelete?: ((view: SavedView) => void) | undefined;
  onRetry: () => void;
}

export function WorkspaceViewsCustoms({
  state,
  views,
  searchHidesRows,
  workspaceSlug,
  onEdit,
  onDelete,
  onRetry,
}: WorkspaceViewsCustomsProps): React.ReactElement | null {
  if (state === "loading") return <ProjectViewsSkeleton />;
  if (state === "error") return <ProjectViewsError onRetry={onRetry} />;
  if (views.length === 0) {
    if (!searchHidesRows) return null;
    return (
      <p
        data-testid="workspace-views-customs-no-match"
        className="text-meta px-(--space-4) py-(--space-3) text-(--text-muted)"
      >
        <Search size={12} strokeWidth={2} aria-hidden="true" className="mr-(--space-1) inline" />
        No views match your search.
      </p>
    );
  }
  return (
    <ul data-testid="workspace-views-customs" className="flex min-w-0 flex-col">
      {views.map((view) => (
        <WorkspaceViewRow
          key={view.id}
          workspaceSlug={workspaceSlug}
          view={view}
          onEdit={onEdit === undefined ? undefined : () => onEdit(view)}
          onDelete={onDelete === undefined ? undefined : () => onDelete(view)}
        />
      ))}
    </ul>
  );
}
