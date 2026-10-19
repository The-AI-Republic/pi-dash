// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The sub-work-items collapsible (parity/specs/issues.md §15, ISS-173/174/
// 175/176/177/178/238). Its title carries the chevron, the kind's label,
// the circular gauge and the full-set done count; the title actions hold
// the display and filter dropdowns plus the add menu. The body draws the
// loaded page through the per-parent filters, grouping and ordering, a
// filtered-empty state with a clear action, a load-more row while another
// cursor page exists, and the create/add-existing/delete dialogs. Every
// parent keeps its own open flag and settings (store.ts).

import { Button, Dialog, IconButton, Menu, Spinner, type MenuEntry } from "@pidash/kit";
import type { Issue, IssuePatch, ProjectDetails, SubIssueItem } from "@pidash/api-client";
import { ChevronRight, ListFilter, Plus, Workflow } from "lucide-react";
import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { FilterSectionOption } from "../filters/shared/FilterSection.js";
import {
  usePickerProjectMembers,
  usePickerStates,
  usePickerWorkspaceMembers,
} from "../../../shared/pickers/reference.js";
import { selectMemberOptions } from "../../../shared/pickers/members.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { WorkItemCreateDialog, type WorkItemCreateProject } from "../components/WorkItemCreateDialog.js";
import { AddExistingDialog } from "./AddExistingDialog.js";
import { useDeleteSubIssue, usePatchSubIssue, useRemoveSubIssue } from "./mutations.js";
import { loadedRows, latestEnvelope, useSubIssues, type SubIssueScope } from "./queries.js";
import { SubIssueDisplayMenu } from "./SubIssueDisplayMenu.js";
import { SubIssueFilterMenu } from "./SubIssueFilterMenu.js";
import { SubIssueRow, type SubIssueRowMenuActions } from "./SubIssueRow.js";
import {
  filterSubIssues,
  groupSubIssues,
  hasActiveFilters,
  subIssueRollup,
  subIssueTitle,
  SUB_ISSUE_FILTER_KEYS,
} from "./rules.js";
import { useSubIssuesUiStore } from "./store.js";
import type { SubIssueFilterSources, SubIssueRowActions, SubIssueWrite } from "./types.js";

/** Title the row menu copies under. */
export const SUB_ISSUE_LINK_COPIED_TITLE = "Link copied";
export const SUB_ISSUE_LINK_COPY_FAILED_TITLE = "Couldn't copy the link";
/** Confirmation before a child is destroyed. */
export const SUB_ISSUE_DELETE_CONFIRM_TITLE = "Delete this work item?";
export function deleteConfirmBody(name: string): string {
  return `"${name}" will be deleted for everyone. This cannot be undone.`;
}
/** The filtered-empty body, per parent kind. */
export function filteredEmptyBody(isEpic: boolean): string {
  return isEpic
    ? "No work items match the filters you've applied."
    : "No sub-work items match the filters you've applied.";
}

/** Browser URL for one child, built from its project code. */
export function subIssueHref(workspaceSlug: string, projectIdentifier: string, row: SubIssueItem): string {
  return `/${workspaceSlug}/browse/${projectIdentifier}-${row.sequence_id}`;
}

/** A ring showing the completed share of the full child set. */
export function SubIssueGauge({ percent, label }: { percent: number; label: string }): React.ReactElement {
  const radius = 7;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference - (Math.min(100, Math.max(0, percent)) / 100) * circumference;
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox="0 0 18 18"
      width={18}
      height={18}
      className="shrink-0 -rotate-90"
      data-percent={percent}
    >
      <circle cx="9" cy="9" r={radius} fill="none" stroke="var(--border)" strokeWidth={3} />
      <circle
        cx="9"
        cy="9"
        r={radius}
        fill="none"
        stroke="var(--accent)"
        strokeWidth={3}
        strokeDasharray={circumference}
        strokeDashoffset={offset}
        strokeLinecap="round"
      />
    </svg>
  );
}

/** Assignee rows for the filter menu, derived from the parent's members. */
export function useAssigneeFilterRows(
  workspaceSlug: string,
  projectId: string,
  enabled: boolean
): SubIssueFilterSources {
  const members = usePickerProjectMembers(workspaceSlug, projectId, enabled);
  const directory = usePickerWorkspaceMembers(workspaceSlug, enabled);
  const me = useMe();
  const rows = React.useMemo(() => {
    if (members.data === undefined || directory.data === undefined) return [];
    const options = selectMemberOptions({
      projectMembers: members.data,
      directory: directory.data,
      meId: me.data?.id ?? null,
      selectedIds: [],
    });
    return options.map(
      (option): FilterSectionOption => ({
        id: option.userId,
        label: option.label,
        searchText: option.searchText,
        disabled: option.suspended,
      })
    );
  }, [directory.data, members.data, me.data?.id]);
  return { assigneeRows: rows, assigneesLoading: enabled && (members.isPending || directory.isPending) };
}

export interface SubIssueSectionProps {
  workspaceSlug: string;
  issue: Issue;
  project: ProjectDetails | undefined;
  editable: boolean;
  /** The project's short code for identifiers and links. */
  projectIdentifier: string;
  /** Guest standing removes the write entries from the row menus. */
  canWrite: boolean;
  /** Opens a child (peek lands with NEWFRONT-646). */
  onOpen: (row: SubIssueItem) => void;
  /** Project rows the create dialog may file under. */
  createProjects: WorkItemCreateProject[];
  /** Title override; the root widget passes the kind's label. */
  heading?: string;
}

/** What the root widget's action row reaches through its ref. */
export interface SubIssueSectionHandle {
  openCreate: () => void;
  openExisting: () => void;
}

export const SubIssueSection = React.forwardRef<SubIssueSectionHandle, SubIssueSectionProps>(function SubIssueSection(
  {
    workspaceSlug,
    issue,
    project,
    editable,
    projectIdentifier,
    canWrite,
    onOpen,
    createProjects,
    heading,
  }: SubIssueSectionProps,
  handle
): React.ReactElement | null {
  const parentId = issue.id;
  const scope: SubIssueScope = { workspaceSlug, projectId: issue.project_id, parentId };
  const state = useSubIssuesUiStore((store) => store.stateFor(parentId));
  const setOpen = useSubIssuesUiStore((store) => store.setOpen);
  const toggleFilter = useSubIssuesUiStore((store) => store.toggleFilter);
  const setFilterValues = useSubIssuesUiStore((store) => store.setFilterValues);
  const resetFilters = useSubIssuesUiStore((store) => store.resetFilters);
  const setGroupBy = useSubIssuesUiStore((store) => store.setGroupBy);
  const setOrderBy = useSubIssuesUiStore((store) => store.setOrderBy);
  const toggleProperty = useSubIssuesUiStore((store) => store.toggleProperty);
  const [addOpen, setAddOpen] = React.useState(false);
  const [createOpen, setCreateOpen] = React.useState(false);
  const [existingOpen, setExistingOpen] = React.useState(false);
  const [pendingDelete, setPendingDelete] = React.useState<SubIssueItem | null>(null);
  const [busyRows, setBusyRows] = React.useState<readonly string[]>([]);
  const query = useSubIssues(workspaceSlug, issue.project_id, parentId, { enabled: state.open });
  const patch = usePatchSubIssue(scope);
  const remove = useRemoveSubIssue(scope);
  const destroy = useDeleteSubIssue(scope);
  const states = usePickerStates(workspaceSlug, issue.project_id, state.open);
  const today = React.useMemo(() => new Date(), []);
  const sources = useAssigneeFilterRows(workspaceSlug, issue.project_id, state.open);
  const isEpic = issue.is_epic ?? false;

  React.useImperativeHandle(
    handle,
    () => ({
      openCreate: () => setCreateOpen(true),
      openExisting: () => setExistingOpen(true),
    }),
    []
  );

  const envelope = latestEnvelope(query.data);
  const rows = loadedRows(query.data);
  const filtered = React.useMemo(() => filterSubIssues(rows, state.filters, today), [rows, state.filters, today]);
  const families = React.useMemo(
    () =>
      groupSubIssues(filtered, state.display.groupBy, state.display.orderBy, {
        states: states.data,
        memberIds: sources.assigneeRows.map((row) => row.id),
        memberNames: Object.fromEntries(sources.assigneeRows.map((row) => [row.id, row.label])),
      }),
    [filtered, state.display.groupBy, state.display.orderBy, states.data, sources.assigneeRows]
  );

  function markBusy(rowId: string, on: boolean): void {
    setBusyRows((current) => (on ? [...new Set([...current, rowId])] : current.filter((id) => id !== rowId)));
  }

  const rowActions: SubIssueRowActions = {
    isBusy: (rowId) => busyRows.includes(rowId) || (patch.isPending && patch.variables?.row.id === rowId),
    write: (input: SubIssueWrite) => {
      if (!editable) return;
      markBusy(input.row.id, true);
      patch.mutate(
        { row: input.row, patch: input.patch as IssuePatch },
        { onSettled: () => markBusy(input.row.id, false) }
      );
    },
  };

  function copyLink(row: SubIssueItem): void {
    const url = `${globalThis.location?.origin ?? ""}/${workspaceSlug}/browse/${projectIdentifier}-${row.sequence_id}`;
    void globalThis.navigator?.clipboard?.writeText(url).then(
      () => notifySuccess(SUB_ISSUE_LINK_COPIED_TITLE),
      () => notifyError(SUB_ISSUE_LINK_COPY_FAILED_TITLE, "Copy the address from the work item instead.")
    );
  }

  const menuActions: SubIssueRowMenuActions = {
    copyLink,
    openInNewTab: (row) => {
      globalThis.open?.(subIssueHref(workspaceSlug, projectIdentifier, row), "_blank", "noopener,noreferrer");
    },
    edit: onOpen,
    remove: (row) => remove.mutate(row),
    delete: (row) => setPendingDelete(row),
  };

  if (envelope === undefined) return null;
  if (subIssueRollup(envelope).total === 0) return null;

  const rollup = subIssueRollup(envelope);
  const title = heading ?? subIssueTitle(isEpic);
  const offered = SUB_ISSUE_FILTER_KEYS.filter(
    (key) => key !== "project" || rows.some((row) => row.project_id !== issue.project_id)
  );

  const addEntries: MenuEntry[] = [
    {
      label: "Create new",
      onSelect: () => setCreateOpen(true),
      icon: <Plus size={14} strokeWidth={1.5} aria-hidden="true" />,
    },
    {
      label: "Add existing",
      onSelect: () => setExistingOpen(true),
      icon: <Workflow size={14} strokeWidth={1.5} aria-hidden="true" />,
    },
  ];

  return (
    <div className="flex flex-col gap-(--space-3)" data-testid="sub-issues-section">
      <div className="flex items-center gap-(--space-2)">
        <button
          type="button"
          className="text-emphasis flex w-full min-w-0 items-center gap-(--space-2) text-left text-(--text)"
          aria-expanded={state.open}
          aria-controls="sub-issues-panel"
          onClick={() => setOpen(parentId, !state.open)}
          data-testid="sub-issues-collapsible"
        >
          <ChevronRight
            size={14}
            strokeWidth={2}
            aria-hidden="true"
            className={state.open ? "rotate-90 transition-transform" : "transition-transform"}
          />
          <SubIssueGauge percent={rollup.percent} label={`${rollup.percent}% done`} />
          <span className="truncate text-14">{title}</span>
          <span className="text-caption shrink-0 text-(--text-muted)">
            {rollup.done}/{rollup.total} Done
          </span>
        </button>
        <span className="flex shrink-0 items-center gap-(--space-1)" onClick={(event) => event.stopPropagation()}>
          <SubIssueDisplayMenu
            display={state.display}
            onGroupBy={(groupBy) => setGroupBy(parentId, groupBy)}
            onOrderBy={(orderBy) => setOrderBy(parentId, orderBy)}
            onToggleProperty={(key) => toggleProperty(parentId, key)}
          />
          <SubIssueFilterMenu
            workspaceSlug={workspaceSlug}
            projectId={issue.project_id}
            filters={state.filters}
            sources={sources}
            offered={offered}
            onToggle={(key, value) => toggleFilter(parentId, key, value)}
            onSetDate={(key, values) => setFilterValues(parentId, key, values)}
          />
          {canWrite ? (
            <Menu
              open={addOpen}
              onOpenChange={setAddOpen}
              trigger={
                <IconButton label="Add a sub-work item" size="small" variant="ghost" data-testid="sub-issues-add">
                  <Plus size={14} strokeWidth={2} aria-hidden="true" />
                </IconButton>
              }
              items={addEntries}
            />
          ) : null}
        </span>
      </div>
      {state.open ? (
        <div id="sub-issues-panel" className="grid overflow-hidden">
          <div className="flex min-h-0 flex-col">
            {query.isPending ? (
              <p role="status" className="text-body px-(--space-2) py-(--space-1) text-(--text-muted)">
                Loading sub-work items
              </p>
            ) : query.isError ? (
              <div className="flex items-center gap-(--space-2) px-(--space-2) py-(--space-1)">
                <span className="text-body text-(--danger)">Couldn&apos;t load the sub-work items.</span>
                <Button variant="ghost" size="small" onClick={() => void query.refetch()}>
                  Try again
                </Button>
              </div>
            ) : filtered.length === 0 && hasActiveFilters(state.filters) ? (
              <div className="flex flex-col items-start gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-3)">
                <span className="flex items-center gap-(--space-2) text-(--text-muted)">
                  <ListFilter size={14} strokeWidth={1.5} aria-hidden="true" />
                  <span className="text-body">{filteredEmptyBody(isEpic)}</span>
                </span>
                <Button variant="secondary" size="small" onClick={() => resetFilters(parentId)}>
                  Clear filters
                </Button>
              </div>
            ) : (
              <div className="flex flex-col">
                {families.map((group) => (
                  <div key={group.id === "" ? "all" : group.id} className="flex flex-col">
                    {state.display.groupBy === null ? null : (
                      <div
                        data-testid="sub-issue-group-header"
                        className="flex items-center gap-(--space-2) px-(--space-2) py-(--space-1)"
                      >
                        <span className="text-caption font-medium text-(--text-muted)">
                          {group.label === null ? "No value" : group.label}
                        </span>
                        <span className="text-meta text-(--text-muted)">{group.rows.length}</span>
                      </div>
                    )}
                    {group.rows.map((row) => (
                      <SubIssueRow
                        key={row.id}
                        row={row}
                        workspaceSlug={workspaceSlug}
                        projectIdentifier={projectIdentifier}
                        properties={state.display.properties}
                        editable={editable}
                        depth={0}
                        ancestors={[parentId]}
                        isEpic={isEpic}
                        actions={rowActions}
                        onOpen={onOpen}
                        menuActions={menuActions}
                      />
                    ))}
                  </div>
                ))}
                {query.hasNextPage ? (
                  <button
                    type="button"
                    className="text-body w-full px-(--space-3) py-(--space-1) text-left font-medium text-(--accent)"
                    disabled={query.isFetchingNextPage}
                    onClick={() => void query.fetchNextPage()}
                    data-testid="sub-issues-load-more"
                  >
                    {query.isFetchingNextPage ? (
                      <span className="flex items-center gap-(--space-2)">
                        <Spinner size="small" aria-label="Loading more sub-work items" />
                        Loading more…
                      </span>
                    ) : (
                      "Load more"
                    )}
                  </button>
                ) : null}
              </div>
            )}
          </div>
        </div>
      ) : null}
      {canWrite ? (
        <>
          <WorkItemCreateDialog
            open={createOpen}
            onOpenChange={setCreateOpen}
            workspaceSlug={workspaceSlug}
            projects={createProjects}
            initialProjectId={issue.project_id}
            lockedProjectId={issue.project_id}
            initialParent={{
              id: parentId,
              name: issue.name,
              identifier: projectIdentifier,
              sequenceId: issue.sequence_id,
            }}
          />
          <AddExistingDialog
            open={existingOpen}
            onOpenChange={setExistingOpen}
            scope={scope}
            projectName={project?.name ?? projectIdentifier}
          />
          <Dialog
            open={pendingDelete !== null}
            onOpenChange={(next) => {
              if (!next) setPendingDelete(null);
            }}
            title={SUB_ISSUE_DELETE_CONFIRM_TITLE}
            actions={
              <>
                <Button variant="ghost" onClick={() => setPendingDelete(null)} disabled={destroy.isPending}>
                  Cancel
                </Button>
                <Button
                  variant="danger"
                  disabled={destroy.isPending}
                  onClick={() => {
                    const row = pendingDelete;
                    if (row === null) return;
                    destroy.mutate(row, { onSettled: () => setPendingDelete(null) });
                  }}
                >
                  Delete
                </Button>
              </>
            }
          >
            <p className="text-body text-(--text-muted)">
              {pendingDelete === null ? "" : deleteConfirmBody(pendingDelete.name)}
            </p>
          </Dialog>
        </>
      ) : null}
    </div>
  );
});
