// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-entity issues layout root (parity/specs/issues.md §1, ISS-001–006).
// Project, cycle, module, saved view, archived and global lists each mount
// one of these: it reads the entity's persisted layout, renders the header
// switcher plus the compact mobile bar, routes empty counts to the empty
// states, floats the mutation spinner while display writes settle, and
// hosts the row-highlight state. Layout slices register their renderer;
// archived contexts offer only list.

import { useSuspenseQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { IssuesHeader } from "../components/IssuesHeader.js";
import { MobileIssuesHeader } from "../components/MobileIssuesHeader.js";
import { HighlightProvider } from "../freshness/highlight.js";
import { LayoutBody } from "../loaders/LayoutBody.js";
import { IssuesLoaderProvider, useIssuesLoader } from "../loaders/loaderContext.js";
import { MutationSpinner } from "../loaders/MutationSpinner.js";
import {
  selectDisplayPrefs,
  switchLayoutPrefs,
  useUpdateDisplayPrefs,
  type DisplayPrefs,
} from "../prefs/displayPrefs.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { resolveActiveLayout, type LayoutKind } from "./kinds.js";

/** Render one layout; layouts read grouping and display from prefs. */
export type LayoutRenderer = (layout: LayoutKind, prefs: DisplayPrefs) => React.ReactNode;

export interface IssuesLayoutRootProps {
  workspaceSlug: string;
  entity: IssuesEntity;
  /** Entity title, e.g. the project name. */
  title: string;
  /** Server total across all pages. */
  totalCount: number;
  /** Rows visible after filtering. */
  resultCount: number;
  /** Registered layouts; archived contexts pass list only. */
  offered: readonly LayoutKind[];
  /**
   * Display settings the root renders from. A saved view passes its own
   * stored record; every other list reads the entity's user-properties.
   */
  display?: DisplayPrefs | undefined;
  /**
   * Where a header switch goes. Absent persists to the entity record; a
   * saved view passes a callback so the switch edits the view locally.
   */
  onDisplayChange?: ((next: LayoutKind) => void) | undefined;
  renderLayout: LayoutRenderer;
  /** Zero-total empty state (context copy arrives with ISS-068). */
  emptyState: React.ReactNode;
  /** Filtered-to-zero empty state. */
  noMatchState: React.ReactNode;
  /** Trailing desktop controls from later slices. */
  headerActions?: React.ReactNode;
  /** Rich-filter row, mounted directly beneath both headers. */
  filterRow?: React.ReactNode;
  /** Display control from the display slice (ISS-075). */
  displayControl?: React.ReactNode;
  /** Rich-filter control from the filter slice (ISS-096). */
  filterControl?: React.ReactNode;
  /** Analytics entry from the filter-inputs slice (ISS-107). */
  analyticsControl?: React.ReactNode;
  /**
   * Hide the layout switcher on both headers while still rendering the
   * resolved layout (a locked saved view, parity/specs/views.md §1).
   */
  hideSwitcher?: boolean | undefined;
}

export function IssuesLayoutRoot(props: IssuesLayoutRootProps): React.ReactElement {
  return (
    <IssuesLoaderProvider>
      <HighlightProvider>
        <IssuesLayoutRootInner {...props} />
      </HighlightProvider>
    </IssuesLoaderProvider>
  );
}

function IssuesLayoutRootInner({
  workspaceSlug,
  entity,
  title,
  totalCount,
  resultCount,
  offered,
  display,
  onDisplayChange,
  renderLayout,
  emptyState,
  noMatchState,
  headerActions,
  filterRow,
  displayControl,
  filterControl,
  analyticsControl,
  hideSwitcher,
}: IssuesLayoutRootProps): React.ReactElement {
  const { data: record } = useSuspenseQuery(prefsQueryOptions(getAppClient(), workspaceSlug, entity));
  const prefs = display ?? selectDisplayPrefs(record);
  const active = resolveActiveLayout(prefs.layout, offered);
  const { updateDisplayPrefs } = useUpdateDisplayPrefs(workspaceSlug, entity);
  const { isMutationLoading } = useIssuesLoader();
  const handleSwitch = (kind: LayoutKind): void => {
    if (onDisplayChange !== undefined) onDisplayChange(kind);
    else updateDisplayPrefs(switchLayoutPrefs(prefs, kind));
  };
  return (
    <div className="flex h-full flex-col">
      <IssuesHeader
        title={title}
        totalCount={totalCount}
        offered={offered}
        active={active}
        onSwitch={handleSwitch}
        actions={
          <>
            {filterControl}
            {analyticsControl}
            {headerActions}
          </>
        }
        hideSwitcher={hideSwitcher}
      />
      <MobileIssuesHeader
        offered={offered}
        active={active}
        onSwitch={handleSwitch}
        displayControl={displayControl}
        filterControl={filterControl}
        analyticsControl={analyticsControl}
        hideSwitcher={hideSwitcher}
      />
      {filterRow}
      {isMutationLoading ? <MutationSpinner /> : null}
      <div className="min-h-0 flex-1">
        <LayoutBody
          layout={active}
          totalCount={totalCount}
          resultCount={resultCount}
          emptyState={emptyState}
          noMatchState={noMatchState}
        >
          {renderLayout(active, prefs)}
        </LayoutBody>
      </div>
    </div>
  );
}
