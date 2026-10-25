// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view layout body (parity/specs/views.md §7, VIEW-028). A view
// stores which of the five issue layouts it opens with; the views detail
// page hands that record here and this module mounts the issues layout
// component with the view's stored expression and display settings.
//
// Every layout rides its own chunk. The detail route starts the chunk for
// the stored layout only (`preloadViewLayout`), so a list-first visit never
// pays for the calendar, the sheet or the timeline; switching layout later
// suspends into the same skeleton. Grouping, paging, skeletons, row actions
// and creation inside each layout stay issues-owned behavior.

import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import type { RichExpression } from "../filters/rich/expression.js";
import { ListSkeleton } from "../loaders/ListSkeleton.js";
import type { PeekRequest } from "../peek/peekUrl.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import type { LayoutKind } from "./kinds.js";

type ListLayoutModule = typeof import("../list/ListLayout.js");
type KanbanBoardModule = typeof import("../kanban/KanbanBoard.js");
type CalendarEntryModule = typeof import("../calendar/CalendarEntry.js");
type SpreadsheetModule = typeof import("../spreadsheet/SpreadsheetLayout.js");
type GanttModule = typeof import("../gantt/GanttTimeline.js");

/** Chunk loaders keyed by the stored layout; each resolves once. */
const layoutLoaders: Record<LayoutKind, () => Promise<unknown>> = {
  list: () => import("../list/ListLayout.js"),
  kanban: () => import("../kanban/KanbanBoard.js"),
  calendar: () => import("../calendar/CalendarEntry.js"),
  spreadsheet: () => import("../spreadsheet/SpreadsheetLayout.js"),
  gantt_chart: () => import("../gantt/GanttTimeline.js"),
};

/**
 * Start the chunk behind one stored layout. The detail route awaits this
 * with its data read, so the first commit is the real layout rather than
 * a skeleton that immediately re-mounts it.
 */
export function preloadViewLayout(layout: LayoutKind): Promise<unknown> {
  return layoutLoaders[layout]();
}

const LazyListLayout = React.lazy(async () => {
  const module: ListLayoutModule = await import("../list/ListLayout.js");
  return { default: module.ListLayout };
});
const LazyKanbanBoard = React.lazy(async () => {
  const module: KanbanBoardModule = await import("../kanban/KanbanBoard.js");
  return { default: module.KanbanBoard };
});
const LazyCalendarEntry = React.lazy(async () => {
  const module: CalendarEntryModule = await import("../calendar/CalendarEntry.js");
  return { default: module.CalendarEntry };
});
const LazySpreadsheetLayout = React.lazy(async () => {
  const module: SpreadsheetModule = await import("../spreadsheet/SpreadsheetLayout.js");
  return { default: module.SpreadsheetLayout };
});
const LazyGanttTimeline = React.lazy(async () => {
  const module: GanttModule = await import("../gantt/GanttTimeline.js");
  return { default: module.GanttTimeline };
});

export interface ViewLayoutBodyProps {
  /** Layout the view record stores. */
  layout: LayoutKind;
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  entity: IssuesEntity;
  /** The view's stored display settings, not the viewer's personal ones. */
  prefs: DisplayPrefs;
  /** Project-aware edit standing from the route. */
  canEdit: boolean;
  /** The view's stored expression scopes every layout. */
  richExpression: RichExpression;
  /** Project feature flags gate the cycle and module entries. */
  hideCycle: boolean;
  hideModule: boolean;
  hideEstimate: boolean;
  /** The profile's first day of week for the sheet and calendar. */
  weekStart: number;
  /** Flat rows the timeline draws; the other layouts page their own reads. */
  issues: readonly IssueListItem[];
  totalCount: number;
  /** Compact density from the route search. */
  compact: boolean;
  /**
   * Peek address the view's route owns (ISS-167). Every layout receives it,
   * so a row in any of the five opens the same URL-bound peek.
   */
  peek?: ViewPeekAddress | undefined;
}

/** Controlled peek params a saved view hands each layout. */
export interface ViewPeekAddress {
  peekKey: string | null;
  peekProjectId: string | null;
  onPeekChange: (request: PeekRequest) => void;
}

export function ViewLayoutBody(props: ViewLayoutBodyProps): React.ReactElement {
  const { layout, peek } = props;
  const peekProps = {
    peekKey: peek?.peekKey,
    ...(peek === undefined ? {} : { onPeekChange: peek.onPeekChange }),
    peekProjectId: peek?.peekProjectId,
  };
  return (
    <React.Suspense fallback={<ListSkeleton />}>
      {layout === "kanban" ? (
        <LazyKanbanBoard
          workspaceSlug={props.workspaceSlug}
          entity={props.entity}
          projectId={props.projectId}
          projectIdentifier={props.projectIdentifier}
          prefs={props.prefs}
          canEdit={props.canEdit}
          richFilters={props.richExpression}
          {...peekProps}
        />
      ) : layout === "calendar" ? (
        <LazyCalendarEntry
          workspaceSlug={props.workspaceSlug}
          projectId={props.projectId}
          projectIdentifier={props.projectIdentifier}
          entity={props.entity}
          canEdit={props.canEdit}
          richExpression={props.richExpression}
          {...peekProps}
        />
      ) : layout === "spreadsheet" ? (
        <LazySpreadsheetLayout
          workspaceSlug={props.workspaceSlug}
          projectId={props.projectId}
          projectIdentifier={props.projectIdentifier}
          entity={props.entity}
          prefs={props.prefs}
          canEdit={props.canEdit}
          hideCycle={props.hideCycle}
          hideModule={props.hideModule}
          hideEstimate={props.hideEstimate}
          richExpression={props.richExpression}
          weekStart={props.weekStart}
          {...peekProps}
        />
      ) : layout === "gantt_chart" ? (
        <LazyGanttTimeline
          workspaceSlug={props.workspaceSlug}
          projectId={props.projectId}
          projectIdentifier={props.projectIdentifier}
          issues={[...props.issues]}
          totalCount={props.totalCount}
          prefs={props.prefs}
          {...peekProps}
        />
      ) : (
        <LazyListLayout
          workspaceSlug={props.workspaceSlug}
          entity={props.entity}
          projectId={props.projectId}
          projectIdentifier={props.projectIdentifier}
          prefs={props.prefs}
          canEdit={props.canEdit}
          richFilters={props.richExpression}
          compact={props.compact}
          {...peekProps}
        />
      )}
    </React.Suspense>
  );
}
