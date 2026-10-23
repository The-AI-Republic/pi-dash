// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List pane (parity/specs/intake.md INT-001/004/005/008). Owns the panel
// the tabs control, the tabs, and exactly one body state: a loading
// skeleton for the first page, the empty state that fits the situation,
// or the fetched requests as selectable links. Row content beyond the
// title (status, dates, priority, labels, author) is INT-006, slice B;
// title links are the minimum this frame needs for selection and
// auto-selection.

import { Skeleton } from "@pidash/kit";
import type { IntakeIssueRow } from "@pidash/api-client";
import * as React from "react";

import type { IntakeEmptyState as IntakeEmptyKind } from "../lib/emptyStates.js";
import type { IntakeTab } from "../lib/tabs.js";
import { IntakeEmptyState } from "./IntakeEmptyState.js";
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
}: IntakeListPaneProps): React.ReactElement {
  return (
    <aside className="flex min-h-0 w-80 shrink-0 flex-col border-r border-(--border)">
      <div className="shrink-0 border-b border-(--border) px-(--space-2) py-(--space-2)">
        <IntakeTabs activeTab={tab} openTotal={openTotal} onSelect={onSelectTab} />
      </div>
      <div
        id={INTAKE_LIST_PANEL_ID}
        role="tabpanel"
        aria-label={`${tab === "open" ? "Open" : "Closed"} intake requests`}
        data-testid="intake-list-pane"
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
}: Omit<IntakeListPaneProps, "tab" | "openTotal" | "onSelectTab">): React.ReactElement {
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
  return (
    <ul data-testid="intake-list" className="flex flex-col">
      {rows.map((row) => {
        const selected = row.issue.id === selectedId;
        return (
          <li key={row.id}>
            <button
              type="button"
              data-testid="intake-row"
              aria-current={selected ? "true" : undefined}
              onClick={() => onSelectRow(row.issue.id)}
              className={
                selected
                  ? "flex w-full flex-col gap-(--space-1) border-b border-(--border) bg-(--subtle) px-(--space-4) py-(--space-2) text-left"
                  : "flex w-full flex-col gap-(--space-1) border-b border-(--border) px-(--space-4) py-(--space-2) text-left hover:bg-(--subtle)"
              }
            >
              <h3 data-testid="intake-row-title" className="text-body truncate text-(--text)">
                {row.issue.name}
              </h3>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
