// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy boundary for the issues rich filters (parity/specs/issues.md §9,
// ISS-095–099). The whole builder — header entry, condition row, value
// editors and picker reads — rides this chunk so the route chunk stays
// free of the filter graph. The render prop hands the mounted control and
// row to the layout root; both come from one builder instance. A saved
// view passes its record so the row edits the view rather than the
// personal filter store.

import * as React from "react";
import type { SavedView } from "@pidash/api-client";

import { useRichFilterBuilder, type RichFilterBuilderParts } from "./RichFilterBuilder.js";

export interface IssueFiltersProps {
  workspaceSlug: string;
  projectId: string;
  hideCycle: boolean;
  hideModule: boolean;
  viewsEnabled?: boolean | undefined;
  /** Saved-view record when the list renders that view (ISS-104). */
  view?: SavedView | undefined;
  onViewCreated?: ((view: SavedView) => void) | undefined;
  onViewUpdated?: ((view: SavedView) => void) | undefined;
  children: (parts: RichFilterBuilderParts) => React.ReactNode;
}

export default function IssueFilters({
  workspaceSlug,
  projectId,
  hideCycle,
  hideModule,
  viewsEnabled,
  view,
  onViewCreated,
  onViewUpdated,
  children,
}: IssueFiltersProps): React.ReactElement {
  // Stable identity: the builder resets its local copy whenever the record
  // reference changes, so a fresh wrapper object per render would loop.
  const viewProp = React.useMemo(() => (view === undefined ? undefined : { view, isDefault: false }), [view]);
  const parts = useRichFilterBuilder({
    workspaceSlug,
    projectId,
    entity: { scope: "project", projectId },
    hideCycle,
    hideModule,
    viewsEnabled,
    view: viewProp,
    onViewCreated,
    onViewUpdated,
  });
  return <React.Fragment>{children(parts)}</React.Fragment>;
}
