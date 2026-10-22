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
import { intakeFeatureSettingsHref } from "../lib/paths.js";
import { intakeSearchFor, settleIntakeSearch, type IntakeSearch } from "../lib/search.js";
import type { IntakeTab } from "../lib/tabs.js";
import { intakeDocumentTitle } from "../lib/title.js";
import { IntakeGate } from "./IntakeGate.js";
import { IntakeShell } from "./IntakeShell.js";

export interface IntakeScreenProps {
  workspaceSlug: string;
  projectId: string;
  search: IntakeSearch;
  /** Writes the settled address; the route supplies the router navigation. */
  onSearchChange: (next: IntakeSearch) => void;
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
    if (next !== null) onSearchChange(next);
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

  const firstPagePending = list.isPending || (list.isLoading && pages === undefined);
  const emptyKind = selectIntakeEmptyState({
    tab,
    rowCount: rows.length,
    userFilterCount: 0,
    statusNarrowed: false,
  });

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
      selectedId={search.inboxIssueId}
      onSelectTab={selectTab}
      onSelectRow={selectRow}
      onRetry={retry}
      onCreate={onCreate}
    />
  );
}
