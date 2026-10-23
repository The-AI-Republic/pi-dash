// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List pane (parity/specs/intake.md INT-001/004/005/008/009/011). Owns
// the panel the tabs control: the tabs, the filter and order toolbar,
// the applied chips, and exactly one body state — a loading skeleton
// for the first page, the empty state that fits the situation, or the
// fetched requests as selectable rows with the paging tail below them.
// Nearing the pane's end reads the next page (INT-007).

import { Skeleton } from "@pidash/kit";
import type { IntakeIssueRow, WorkspaceMemberList } from "@pidash/api-client";
import * as React from "react";

import type { IntakeEmptyState as IntakeEmptyKind } from "../lib/emptyStates.js";
import type { IntakeFilterSelection } from "../lib/filters.js";
import type { IntakeMemberOption } from "../lib/members.js";
import type { IntakeOrder } from "../lib/order.js";
import { nearListEnd } from "../lib/paging.js";
import { uniqueIntakeRows } from "../lib/rows.js";
import type { IntakeTab } from "../lib/tabs.js";
import { IntakeChips } from "./IntakeChips.js";
import { IntakeEmptyState } from "./IntakeEmptyState.js";
import { IntakeFilterPanel, type IntakeLabelOption } from "./IntakeFilterPanel.js";
import { IntakeListPager } from "./IntakeListPager.js";
import { IntakeOrderMenu } from "./IntakeOrderMenu.js";
import { IntakeRow } from "./IntakeRow.js";
import { INTAKE_LIST_PANEL_ID, IntakeTabs } from "./IntakeTabs.js";

export interface IntakeListPaneProps {
  tab: IntakeTab;
  openTotal: number | undefined;
  /** True while the active tab's first page is in flight. */
  loading: boolean;
  /** True when the read failed; the pane offers a retry instead of rows. */
  failed: boolean;
  rows: readonly IntakeIssueRow[];
  emptyKind: IntakeEmptyKind;
  selectedId: string | undefined;
  onSelectTab: (tab: IntakeTab) => void;
  onSelectRow: (issueId: string) => void;
  onRetry: () => void;
  onCreate?: (() => void) | undefined;
  /** Project prefix for the row id labels (INT-006). */
  projectIdentifier: string;
  labels: readonly IntakeLabelOption[];
  directory: WorkspaceMemberList | undefined;
  /** Whether the server reports another page (INT-007). */
  hasNextPage: boolean;
  /** True while a page beyond the first is in flight. */
  loadingMore: boolean;
  onLoadMore: () => void;
  selection: IntakeFilterSelection;
  assigneeOptions: readonly IntakeMemberOption[];
  createdByOptions: readonly IntakeMemberOption[];
  membersLoading: boolean;
  membersFailed: boolean;
  labelsLoading: boolean;
  labelsFailed: boolean;
  today: string;
  weekStart: number;
  onSelectionChange: (next: IntakeFilterSelection) => void;
  order: IntakeOrder;
  onOrderChange: (order: IntakeOrder) => void;
}

export function IntakeListPane({
  tab,
  openTotal,
  loading,
  failed,
  rows,
  emptyKind,
  selectedId,
  onSelectTab,
  onSelectRow,
  onRetry,
  onCreate,
  projectIdentifier,
  labels,
  directory,
  hasNextPage,
  loadingMore,
  onLoadMore,
  selection,
  assigneeOptions,
  createdByOptions,
  membersLoading,
  membersFailed,
  labelsLoading,
  labelsFailed,
  today,
  weekStart,
  onSelectionChange,
  order,
  onOrderChange,
}: IntakeListPaneProps): React.ReactElement {
  const labelById = React.useMemo(
    () => new Map(labels.map((label) => [label.id, { name: label.name, color: label.color }])),
    [labels]
  );
  const memberNameById = React.useMemo(
    () => new Map(assigneeOptions.map((option) => [option.userId, option.displayName])),
    [assigneeOptions]
  );
  const labelNameById = React.useMemo(() => new Map(labels.map((label) => [label.id, label.name])), [labels]);

  function handleScroll(event: React.UIEvent<HTMLDivElement>): void {
    if (!hasNextPage || loadingMore) return;
    const pane = event.currentTarget;
    if (nearListEnd({ scrollHeight: pane.scrollHeight, scrollTop: pane.scrollTop, clientHeight: pane.clientHeight })) {
      onLoadMore();
    }
  }

  return (
    <aside className="flex min-h-0 w-80 shrink-0 flex-col border-r border-(--border)">
      <div className="shrink-0 border-b border-(--border) px-(--space-2) py-(--space-2)">
        <IntakeTabs activeTab={tab} openTotal={openTotal} onSelect={onSelectTab} />
      </div>
      <div className="flex shrink-0 items-center gap-(--space-1) px-(--space-2) py-(--space-1)">
        <IntakeFilterPanel
          tab={tab}
          selection={selection}
          assigneeOptions={assigneeOptions}
          createdByOptions={createdByOptions}
          membersLoading={membersLoading}
          membersFailed={membersFailed}
          labelOptions={labels}
          labelsLoading={labelsLoading}
          labelsFailed={labelsFailed}
          today={today}
          weekStart={weekStart}
          onSelectionChange={onSelectionChange}
        />
        <IntakeOrderMenu order={order} onChange={onOrderChange} />
      </div>
      <div className="shrink-0 px-(--space-4) py-(--space-1)">
        <IntakeChips
          selection={selection}
          memberNameById={memberNameById}
          labelNameById={labelNameById}
          onSelectionChange={onSelectionChange}
        />
      </div>
      <div
        id={INTAKE_LIST_PANEL_ID}
        role="tabpanel"
        aria-label={`${tab === "open" ? "Open" : "Closed"} intake requests`}
        data-testid="intake-list-pane"
        tabIndex={-1}
        onScroll={handleScroll}
        className="min-h-0 flex-1 overflow-y-auto"
      >
        <IntakeListBody
          loading={loading}
          failed={failed}
          rows={rows}
          emptyKind={emptyKind}
          selectedId={selectedId}
          onSelectRow={onSelectRow}
          onRetry={onRetry}
          onCreate={onCreate}
          projectIdentifier={projectIdentifier}
          labelById={labelById}
          directory={directory}
          hasNextPage={hasNextPage}
        />
      </div>
    </aside>
  );
}

function IntakeListBody({
  loading,
  failed,
  rows,
  emptyKind,
  selectedId,
  onSelectRow,
  onRetry,
  onCreate,
  projectIdentifier,
  labelById,
  directory,
  hasNextPage,
}: {
  loading: boolean;
  failed: boolean;
  rows: readonly IntakeIssueRow[];
  emptyKind: IntakeEmptyKind;
  selectedId: string | undefined;
  onSelectRow: (issueId: string) => void;
  onRetry: () => void;
  onCreate?: (() => void) | undefined;
  projectIdentifier: string;
  labelById: ReadonlyMap<string, { name: string; color: string }>;
  directory: WorkspaceMemberList | undefined;
  hasNextPage: boolean;
}): React.ReactElement {
  if (loading) {
    return (
      <div data-testid="intake-list-loading" className="flex flex-col">
        {Array.from({ length: 5 }, (_, index) => (
          <div key={index} className="flex h-11 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)">
            <Skeleton height="14px" />
          </div>
        ))}
      </div>
    );
  }
  if (failed) {
    return (
      <div data-testid="intake-list-error" className="flex flex-col gap-(--space-2) p-(--space-6)">
        <h3 className="text-emphasis text-(--text)">The requests could not be loaded.</h3>
        <button type="button" className="text-body self-start text-(--accent) underline" onClick={onRetry}>
          Retry
        </button>
      </div>
    );
  }
  if (rows.length === 0) return <IntakeEmptyState kind={emptyKind} onCreate={onCreate} />;
  const unique = uniqueIntakeRows(rows);
  return (
    <>
      <ul data-testid="intake-list" className="flex flex-col">
        {unique.map((row) => (
          <li key={row.id}>
            <IntakeRow
              row={row}
              projectIdentifier={projectIdentifier}
              labelById={labelById}
              directory={directory}
              selected={row.issue.id === selectedId}
              onSelect={onSelectRow}
            />
          </li>
        ))}
      </ul>
      <IntakeListPager hasNextPage={hasNextPage} />
    </>
  );
}
