// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake screen (parity/specs/intake.md INT-001–012, 030, 031). Reads the
// project row for the feature gate and the name, then one list read
// for the active tab, filter set and order. The address is the source
// of truth for the tab and the selection: the screen validates what it
// found, keeps a selection that the settled list contains, auto-selects
// the first request when the address names none, and otherwise leaves
// the placeholder in place. Filter values live per project and tab
// session — switching tabs restores that tab's own defaults — and
// every filter or order change refetches from the first page (INT-009,
// INT-011). Nothing here polls, subscribes or re-reads on focus.

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { useMe, useWorkspace } from "../../../core/session/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { useNarrowViewport } from "../../../shared/shell/narrowViewport.js";
import { usePickerProjectMembers, usePickerWorkspaceMembers } from "../../../shared/pickers/index.js";
import { useLabels, useWeekStart } from "../../issues/index.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { DetailSkeleton } from "../../issues/index.js";
import { isProjectAdmin } from "../../projects/index.js";
import { intakeKeys } from "../api/keys.js";
import { intakeTotal, intakeRows, useIntakeList } from "../api/queries.js";
import { todayUtcIsoDay } from "../lib/dates.js";
import { selectIntakeEmptyState } from "../lib/emptyStates.js";
import {
  defaultSelectionForTab,
  filterActivity,
  selectionToListFilters,
  type IntakeFilterSelection,
} from "../lib/filters.js";
import { selectIntakeMemberOptions } from "../lib/members.js";
import { neighborIntakeId } from "../lib/navigation.js";
import { DEFAULT_INTAKE_ORDER_VALUE, orderByForOrder, type IntakeOrder } from "../lib/order.js";
import { uniqueIntakeRows } from "../lib/rows.js";
import { intakeFeatureSettingsHref } from "../lib/paths.js";
import type { IntakeSearch, IntakeSearchWriteOptions } from "../lib/search.js";
import { intakeSearchAfterRemove, intakeSearchFor, settleIntakeSearch, withoutSelection } from "../lib/searchWrites.js";
import type { IntakeTab } from "../lib/tabs.js";
import { intakeDocumentTitle } from "../lib/title.js";
import { IntakeGate } from "./IntakeGate.js";
import { IntakeShell } from "./IntakeShell.js";

type IntakeDetailModule = typeof import("./IntakeDetail.js");

// The detail rides its own chunk (NEWFRONT-870, perf budgets): the route
// prefetches this screen at boot, and the detail's editor, sidebar and
// attachment stack must not travel with it onto every page load. The
// selection mounts it, so a first paint of the pane is the skeleton the
// detail route shows while its own load resolves.
const LazyIntakeDetail = React.lazy(async () => {
  const module: IntakeDetailModule = await import("./IntakeDetail.js");
  return { default: module.IntakeDetail };
});

export interface IntakeScreenProps {
  workspaceSlug: string;
  projectId: string;
  search: IntakeSearch;
  /** Writes the address; user moves push, corrections replace. The route supplies the router navigation. */
  onSearchChange: (next: IntakeSearch, options?: IntakeSearchWriteOptions) => void;
  /** Opens the create dialog; slice C owns the dialog. */
  onCreate?: (() => void) | undefined;
}

export function IntakeScreen({
  workspaceSlug,
  projectId,
  search,
  onSearchChange,
  onCreate,
}: IntakeScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const project = useProjectRow(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);
  const { workspace } = useWorkspace(workspaceSlug);
  const me = useMe();
  const weekStart = useWeekStart();
  const gateOff = project.data?.inbox_view === false;
  const ready = project.isSuccess && !gateOff;
  const [today] = React.useState(() => todayUtcIsoDay());
  // Narrow viewports go single-pane (INT-030): the detail fills the
  // width and the list rides an overlay the mobile header toggles. The
  // overlay starts open; picking a row closes it.
  const narrow = useNarrowViewport();
  const [mobileListOpen, setMobileListOpen] = React.useState(true);

  useDocumentHead({ title: intakeDocumentTitle(project.data?.name), robots: "noindex, nofollow" });

  const tab = search.currentTab;

  // The tab session's filter selection. A new project or tab starts
  // from that tab's defaults; the render below commits the fresh
  // session before anything reads it.
  const sessionKey = `${projectId}:${tab}`;
  const [filterSession, setFilterSession] = React.useState<{
    key: string;
    selection: IntakeFilterSelection;
  } | null>(null);
  const selection = filterSession?.key === sessionKey ? filterSession.selection : defaultSelectionForTab(tab);
  if (filterSession?.key !== sessionKey) {
    setFilterSession({ key: sessionKey, selection });
  }

  // The list order, kept across tabs and reset per project.
  const [orderSession, setOrderSession] = React.useState<{ projectId: string; order: IntakeOrder } | null>(null);
  const order = orderSession?.projectId === projectId ? orderSession.order : DEFAULT_INTAKE_ORDER_VALUE;
  if (orderSession?.projectId !== projectId) {
    setOrderSession({ projectId, order });
  }

  const list = useIntakeList(workspaceSlug, projectId, {
    tab,
    filters: selectionToListFilters(tab, selection, today),
    orderBy: orderByForOrder(order),
    enabled: ready,
  });
  const labelsQuery = useLabels(workspaceSlug, projectId, ready);
  const directoryQuery = usePickerWorkspaceMembers(workspaceSlug, ready);
  const projectMembersQuery = usePickerProjectMembers(workspaceSlug, projectId, ready);

  const meId = me.data?.id ?? null;
  const assigneeOptions = React.useMemo(
    () =>
      selectIntakeMemberOptions({
        projectMembers: projectMembersQuery.data,
        directory: directoryQuery.data,
        meId,
        selectedIds: selection.assignees,
      }),
    [projectMembersQuery.data, directoryQuery.data, meId, selection.assignees]
  );
  const createdByOptions = React.useMemo(
    () =>
      selectIntakeMemberOptions({
        projectMembers: projectMembersQuery.data,
        directory: directoryQuery.data,
        meId,
        selectedIds: selection.createdBy,
      }),
    [projectMembersQuery.data, directoryQuery.data, meId, selection.createdBy]
  );
  const labelOptions = React.useMemo(
    () => (labelsQuery.data ?? []).map((label) => ({ id: label.id, name: label.name, color: label.color })),
    [labelsQuery.data]
  );

  const pages = list.data?.pages;
  const rows = React.useMemo(() => uniqueIntakeRows(intakeRows(pages)), [pages]);
  const openTotal = intakeTotal(pages);
  const rowIds = React.useMemo(() => rows.map((row) => row.issue.id), [rows]);

  // Settle the address once the active tab's first page has landed
  // (INT-005/INT-028). A filter or order change drops a selection the
  // new first page no longer contains through the same path.
  const settled = list.isSuccess;
  React.useEffect(() => {
    if (!settled) return;
    const next = settleIntakeSearch({ search, rowIds });
    if (next !== null) onSearchChange(next, { replace: true });
  }, [settled, search, rowIds, onSearchChange]);

  // Switching tabs refetches with that tab's default status set and drops
  // a selection from the other tab (INT-004/INT-012/INT-028). The switch
  // itself is the fetch trigger: the destination key is invalidated so a
  // list still inside its stale time re-reads (switching back to Open
  // shows requests created while Closed was open).
  const selectTab = React.useCallback(
    (next: IntakeTab) => {
      if (next === tab) return;
      void queryClient.invalidateQueries({ queryKey: intakeKeys.lists(workspaceSlug, projectId) });
      onSearchChange(intakeSearchFor(next, null));
    },
    [onSearchChange, projectId, queryClient, tab, workspaceSlug]
  );
  const selectRow = React.useCallback(
    (issueId: string) => {
      onSearchChange(intakeSearchFor(tab, issueId));
      if (narrow) setMobileListOpen(false);
    },
    [narrow, onSearchChange, tab]
  );
  const toggleMobileList = React.useCallback(() => {
    setMobileListOpen((open) => !open);
  }, []);
  // Previous/next steps (INT-027): the neighbour in display order with
  // wraparound, pushed so back/forward walks them. A step with no rows
  // is a no-op; the detail only mounts while a selection exists.
  const stepSelection = React.useCallback(
    (direction: "prev" | "next") => {
      const nextId = neighborIntakeId(rowIds, search.inboxIssueId, direction);
      if (nextId === null) return;
      onSearchChange(intakeSearchFor(tab, nextId));
    },
    [onSearchChange, rowIds, search.inboxIssueId, tab]
  );
  const stepPrev = React.useCallback(() => stepSelection("prev"), [stepSelection]);
  const stepNext = React.useCallback(() => stepSelection("next"), [stepSelection]);
  // A triage move took the request out of the tab (INT-020–025): the
  // neighbour takes the selection — pushed, so back/forward walks the
  // move — and an emptied tab keeps no selection. True when emptied.
  const advanceAfterTriage = React.useCallback(
    (removedId: string) => {
      const next = intakeSearchAfterRemove({ search, rowIds, removedId });
      onSearchChange(next);
      return next.inboxIssueId === undefined;
    },
    [onSearchChange, rowIds, search]
  );
  // A stale or forbidden id (INT-028 step 3): drop it from the address
  // and re-read the tab, so the settle pass auto-selects a live row.
  const dropMissing = React.useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: intakeKeys.lists(workspaceSlug, projectId) });
    onSearchChange(withoutSelection(search), { replace: true });
  }, [onSearchChange, projectId, queryClient, search, workspaceSlug]);
  const retry = React.useCallback(() => {
    void list.refetch();
  }, [list]);
  // Every filter or order change refetches (INT-009, INT-011): the
  // invalidation marks the destination key's cache entry stale — even
  // one still inside its stale time — so remounting a recently read
  // combination re-reads instead of serving the cache.
  const changeSelection = React.useCallback(
    (next: IntakeFilterSelection) => {
      setFilterSession({ key: sessionKey, selection: next });
      void queryClient.invalidateQueries({ queryKey: intakeKeys.lists(workspaceSlug, projectId) });
    },
    [projectId, queryClient, sessionKey, workspaceSlug]
  );
  const changeOrder = React.useCallback(
    (next: IntakeOrder) => {
      setOrderSession({ projectId, order: next });
      void queryClient.invalidateQueries({ queryKey: intakeKeys.lists(workspaceSlug, projectId) });
    },
    [projectId, queryClient, workspaceSlug]
  );
  const loadMore = React.useCallback(() => {
    void list.fetchNextPage();
  }, [list]);
  const manageFeatures = React.useCallback(() => {
    void navigate({ to: intakeFeatureSettingsHref(workspaceSlug, projectId) });
  }, [navigate, projectId, workspaceSlug]);

  if (!project.isSuccess) {
    return (
      <section aria-label="Intake" data-testid="intake-screen" className="flex min-h-0 flex-1 flex-col p-(--space-4)">
        {project.isError ? (
          <p className="text-body text-(--text-muted)">The project could not be loaded.</p>
        ) : (
          <p className="text-body text-(--text-muted)">Loading intake…</p>
        )}
      </section>
    );
  }

  if (gateOff) {
    return (
      <section aria-label="Intake" data-testid="intake-screen" className="flex min-h-0 flex-1 flex-col p-(--space-4)">
        <IntakeGate
          canManageFeatures={isProjectAdmin(roles.data?.[projectId] ?? null)}
          onManageFeatures={manageFeatures}
        />
      </section>
    );
  }

  // Only a first load shows the skeleton: a background re-read (tab
  // switch invalidation, retry) keeps the rows it already has on screen.
  const firstPagePending = list.isPending;
  const activity = filterActivity(selection, tab);
  const emptyKind = selectIntakeEmptyState({
    tab,
    rowCount: rows.length,
    userFilterCount: activity.userFilterCount,
    statusNarrowed: activity.statusNarrowed,
  });
  // Creation is offered to any project standing while intake is on
  // (INT-013); the gate above already returned for a switched-off
  // project, so only membership is left to check.
  const canCreate = (roles.data?.[projectId] ?? null) !== null;

  const selectedId = search.inboxIssueId;
  // With no selection the mobile header is gone, so the overlay stays
  // open: otherwise a narrow empty tab would strand the list shut with
  // no toggle to reopen it.
  const listOpen = !narrow || selectedId === undefined || mobileListOpen;
  return (
    <IntakeShell
      workspaceSlug={workspaceSlug}
      workspaceName={workspace?.name ?? workspaceSlug}
      projectId={projectId}
      projectName={project.data.name}
      tab={tab}
      openTotal={openTotal}
      loading={firstPagePending}
      failed={list.isError}
      syncing={list.isFetching}
      rows={rows}
      emptyKind={emptyKind}
      selectedId={selectedId}
      onSelectTab={selectTab}
      onSelectRow={selectRow}
      onRetry={retry}
      onCreate={onCreate}
      narrow={narrow}
      mobileListOpen={listOpen}
      detail={
        selectedId === undefined ? undefined : (
          <React.Suspense fallback={<DetailSkeleton />}>
            <LazyIntakeDetail
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issueId={selectedId}
              projectIdentifier={project.data.identifier}
              projectName={project.data.name}
              published={project.data.anchor != null}
              role={roles.data?.[projectId] ?? null}
              onPrev={stepPrev}
              onNext={stepNext}
              narrow={narrow}
              listOpen={listOpen}
              onToggleList={toggleMobileList}
              onMissing={dropMissing}
              onAdvance={advanceAfterTriage}
            />
          </React.Suspense>
        )
      }
      canCreate={canCreate}
      projectIdentifier={project.data.identifier}
      labels={labelOptions}
      directory={directoryQuery.data}
      hasNextPage={list.hasNextPage}
      loadingMore={list.isFetchingNextPage}
      onLoadMore={loadMore}
      selection={selection}
      assigneeOptions={assigneeOptions}
      createdByOptions={createdByOptions}
      membersLoading={projectMembersQuery.isPending || directoryQuery.isPending}
      membersFailed={projectMembersQuery.isError || directoryQuery.isError}
      labelsLoading={labelsQuery.isPending}
      labelsFailed={labelsQuery.isError}
      today={today}
      weekStart={weekStart}
      onSelectionChange={changeSelection}
      order={order}
      onOrderChange={changeOrder}
    />
  );
}
