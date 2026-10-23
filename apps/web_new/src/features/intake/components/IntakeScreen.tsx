// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake screen (parity/specs/intake.md INT-001–005, 008, 012, 031). Reads
// the project row for the feature gate and the name, then one list read
// for the active tab. The address is the source of truth for the tab and
// the selection: the screen validates what it found, keeps a selection
// that the settled list contains, auto-selects the first request when the
// address names none, and otherwise leaves the placeholder in place.
// Nothing here polls, subscribes or re-reads on focus; switching tabs,
// choosing a request and the retry control are the only fetch triggers
// this slice owns.

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { useWorkspace } from "../../../core/session/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { isProjectAdmin } from "../../projects/index.js";
import { intakeKeys } from "../api/keys.js";
import { intakeTotal, intakeRows, useIntakeList } from "../api/queries.js";
import { selectIntakeEmptyState } from "../lib/emptyStates.js";
import { neighborIntakeId } from "../lib/navigation.js";
import { intakeFeatureSettingsHref } from "../lib/paths.js";
import {
  intakeSearchFor,
  settleIntakeSearch,
  withoutSelection,
  type IntakeSearch,
  type IntakeSearchWriteOptions,
} from "../lib/search.js";
import type { IntakeTab } from "../lib/tabs.js";
import { intakeDocumentTitle } from "../lib/title.js";
import { IntakeDetail } from "./IntakeDetail.js";
import { IntakeGate } from "./IntakeGate.js";
import { IntakeShell } from "./IntakeShell.js";

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
  const gateOff = project.data?.inbox_view === false;

  useDocumentHead({ title: intakeDocumentTitle(project.data?.name), robots: "noindex, nofollow" });

  const tab = search.currentTab;
  const list = useIntakeList(workspaceSlug, projectId, { tab, enabled: project.isSuccess && !gateOff });
  const pages = list.data?.pages;
  const rows = intakeRows(pages);
  const openTotal = intakeTotal(pages);
  const rowIds = React.useMemo(() => rows.map((row) => row.issue.id), [rows]);

  // Settle the address once the active tab's first page has landed
  // (INT-005/INT-028). A user filter is not part of this slice, so an
  // empty untouched tab still counts as "no user filters" (INT-008).
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
    },
    [onSearchChange, tab]
  );
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
  // A stale or forbidden id (INT-028 step 3): drop it from the address
  // and re-read the tab, so the settle pass auto-selects a live row.
  const dropMissing = React.useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: intakeKeys.lists(workspaceSlug, projectId) });
    onSearchChange(withoutSelection(search), { replace: true });
  }, [onSearchChange, projectId, queryClient, search, workspaceSlug]);
  const retry = React.useCallback(() => {
    void list.refetch();
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
  const emptyKind = selectIntakeEmptyState({
    tab,
    rowCount: rows.length,
    userFilterCount: 0,
    statusNarrowed: false,
  });

  const selectedId = search.inboxIssueId;
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
      rows={rows}
      emptyKind={emptyKind}
      selectedId={selectedId}
      onSelectTab={selectTab}
      onSelectRow={selectRow}
      onRetry={retry}
      onCreate={onCreate}
      detail={
        selectedId === undefined ? undefined : (
          <IntakeDetail
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            issueId={selectedId}
            projectIdentifier={project.data.identifier}
            role={roles.data?.[projectId] ?? null}
            onPrev={stepPrev}
            onNext={stepNext}
            onMissing={dropMissing}
          />
        )
      }
    />
  );
}
