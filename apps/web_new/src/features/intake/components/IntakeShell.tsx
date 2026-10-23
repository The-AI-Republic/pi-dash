// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake two-pane frame (parity/specs/intake.md INT-001, INT-007). The
// trail and header sit above a body split into the list pane and the
// content pane. The screen supplies the request detail for the content
// pane; with no selection it shows the pick placeholder. While a list
// read is in flight the header shows the syncing hint (INT-007). The
// frame owns layout only; the screen owns reads and the address.

import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import { intakeHref } from "../lib/paths.js";
import { INTAKE_AREA_LABEL } from "../lib/title.js";
import type { IntakeTab } from "../lib/tabs.js";
import { IntakeBreadcrumb } from "./IntakeBreadcrumb.js";
import { IntakeListPane } from "./IntakeListPane.js";
import { IntakePlaceholder } from "./IntakePlaceholder.js";

export interface IntakeShellProps {
  workspaceSlug: string;
  workspaceName: string;
  projectId: string;
  projectName: string;
  tab: IntakeTab;
  openTotal: number | undefined;
  loading: boolean;
  failed: boolean;
  /** True while a list read is in flight; the header hints at it (INT-007). */
  syncing: boolean;
  rows: React.ComponentProps<typeof IntakeListPane>["rows"];
  emptyKind: React.ComponentProps<typeof IntakeListPane>["emptyKind"];
  selectedId: string | undefined;
  onSelectTab: (tab: IntakeTab) => void;
  onSelectRow: (issueId: string) => void;
  onRetry: () => void;
  onCreate?: (() => void) | undefined;
  /** Request detail for the content pane; the placeholder when absent. */
  detail?: React.ReactNode | undefined;
  /**
   * Whether the screen offers creation (INT-013): any project role while
   * intake is on. The header control renders only when this is true.
   */
  canCreate?: boolean | undefined;
  projectIdentifier: React.ComponentProps<typeof IntakeListPane>["projectIdentifier"];
  labels: React.ComponentProps<typeof IntakeListPane>["labels"];
  directory: React.ComponentProps<typeof IntakeListPane>["directory"];
  hasNextPage: React.ComponentProps<typeof IntakeListPane>["hasNextPage"];
  loadingMore: React.ComponentProps<typeof IntakeListPane>["loadingMore"];
  onLoadMore: React.ComponentProps<typeof IntakeListPane>["onLoadMore"];
  selection: React.ComponentProps<typeof IntakeListPane>["selection"];
  assigneeOptions: React.ComponentProps<typeof IntakeListPane>["assigneeOptions"];
  createdByOptions: React.ComponentProps<typeof IntakeListPane>["createdByOptions"];
  membersLoading: React.ComponentProps<typeof IntakeListPane>["membersLoading"];
  membersFailed: React.ComponentProps<typeof IntakeListPane>["membersFailed"];
  labelsLoading: React.ComponentProps<typeof IntakeListPane>["labelsLoading"];
  labelsFailed: React.ComponentProps<typeof IntakeListPane>["labelsFailed"];
  today: React.ComponentProps<typeof IntakeListPane>["today"];
  weekStart: React.ComponentProps<typeof IntakeListPane>["weekStart"];
  onSelectionChange: React.ComponentProps<typeof IntakeListPane>["onSelectionChange"];
  order: React.ComponentProps<typeof IntakeListPane>["order"];
  onOrderChange: React.ComponentProps<typeof IntakeListPane>["onOrderChange"];
}

export function IntakeShell({
  workspaceSlug,
  workspaceName,
  projectId,
  projectName,
  tab,
  openTotal,
  loading,
  failed,
  syncing,
  rows,
  emptyKind,
  selectedId,
  onSelectTab,
  onSelectRow,
  onRetry,
  onCreate,
  detail,
  canCreate = false,
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
}: IntakeShellProps): React.ReactElement {
  return (
    <section
      aria-label={INTAKE_AREA_LABEL}
      data-testid="intake-shell"
      className="flex min-h-0 flex-1 flex-col bg-(--bg) text-(--text)"
    >
      <header className="flex shrink-0 flex-col gap-(--space-2) px-(--space-4) py-(--space-3)">
        <div className="flex items-center gap-(--space-2)">
          <div className="min-w-0 flex-1">
            <IntakeBreadcrumb
              workspaceName={workspaceName}
              projectName={projectName}
              href={intakeHref(workspaceSlug, projectId)}
            />
          </div>
          {syncing ? (
            <p
              data-testid="intake-syncing"
              role="status"
              className="text-meta inline-flex items-center gap-(--space-1) text-(--text-muted)"
            >
              <Spinner size="small" label="Syncing" />
              Syncing
            </p>
          ) : null}
          {canCreate && onCreate !== undefined ? (
            <Button variant="primary" size="small" data-testid="intake-header-create" onClick={onCreate}>
              Create request
            </Button>
          ) : null}
        </div>
      </header>
      <div className="flex min-h-0 flex-1">
        <IntakeListPane
          tab={tab}
          openTotal={openTotal}
          loading={loading}
          failed={failed}
          rows={rows}
          emptyKind={emptyKind}
          selectedId={selectedId}
          onSelectTab={onSelectTab}
          onSelectRow={onSelectRow}
          onRetry={onRetry}
          onCreate={onCreate}
          projectIdentifier={projectIdentifier}
          labels={labels}
          directory={directory}
          hasNextPage={hasNextPage}
          loadingMore={loadingMore}
          onLoadMore={onLoadMore}
          selection={selection}
          assigneeOptions={assigneeOptions}
          createdByOptions={createdByOptions}
          membersLoading={membersLoading}
          membersFailed={membersFailed}
          labelsLoading={labelsLoading}
          labelsFailed={labelsFailed}
          today={today}
          weekStart={weekStart}
          onSelectionChange={onSelectionChange}
          order={order}
          onOrderChange={onOrderChange}
        />
        <div data-testid="intake-content-pane" className="min-h-0 min-w-0 flex-1">
          {detail ?? <IntakePlaceholder />}
        </div>
      </div>
    </section>
  );
}
