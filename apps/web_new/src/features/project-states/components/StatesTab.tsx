// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// States tab body (parity/specs/project-settings-states.md, PSET-028–035).
// Composes the project's cached state list, the editor rights, the
// member-editing toggle and the seven group cards. Loading shows a
// multi-row skeleton before any grouped content (Intended, bug
// NEWFRONT-405); a failed first read is a notice-only state with a retry
// through the tab's own reopen, never an inline error panel. Every
// mutation answers through a toast, and the mutations that paint
// optimistically restore their row when the server refuses.

import { Button, Skeleton } from "@pidash/kit";
import { RefreshCw } from "lucide-react";
import * as React from "react";

import type { State } from "@pidash/api-client";
import { useProjectRow } from "../../auth/index.js";
import { useProjectSettings } from "../../project-settings/components/ProjectSettingsLayout.js";
import {
  useCreateState,
  useDeleteState,
  useMarkStateDefault,
  useMoveState,
  useProjectStates,
  useSetMemberEditing,
  useUpdateState,
} from "../api.js";
import { groupStates, isWorkflowGroup, WORKFLOW_GROUPS, type GroupedStates, type WorkflowGroup } from "../groups.js";
import { resolveStateMove } from "../ordering.js";
import { canEditStates, canToggleMemberEditing } from "../rights.js";
import { MemberEditingToggle } from "./MemberEditingToggle.js";
import { StateDeleteDialog } from "./StateDeleteDialog.js";
import { StateGroupCard, type StateDropTarget } from "./StateGroupCard.js";
import type { StateFormValue } from "./StateForm.js";

/** Rows the loading skeleton sketches before grouped content arrives. */
export const SKELETON_ROWS = 4;

/** Explanation shown beside a delete control that cannot fire. */
export const DELETE_DEFAULT_REASON = "The default state can't be deleted";
export const DELETE_LAST_IN_GROUP_REASON = "A group needs at least one state";

/**
 * Why deletion is refused for a row, or null when it may proceed: the
 * project default always survives, and so does the last state of a
 * group (a group is never emptied from the settings tab).
 */
export function deleteBlockedReason(state: State, rows: readonly State[]): string | null {
  if (state.default) return DELETE_DEFAULT_REASON;
  if (rows.length <= 1) return DELETE_LAST_IN_GROUP_REASON;
  return null;
}

/** Which cards start collapsed; review folds away on load. */
function initialExpanded(): Record<WorkflowGroup, boolean> {
  const expanded = {} as Record<WorkflowGroup, boolean>;
  for (const group of WORKFLOW_GROUPS) expanded[group] = group !== "review";
  return expanded;
}

export function StatesTab(): React.ReactElement {
  const { workspaceSlug, projectId, role } = useProjectSettings();
  const states = useProjectStates(workspaceSlug, projectId);
  const project = useProjectRow(workspaceSlug, projectId);
  const flag = project.data?.members_can_edit_states ?? true;
  const editable = canEditStates(role, flag);
  const showToggle = canToggleMemberEditing(role);

  const create = useCreateState();
  const update = useUpdateState();
  const move = useMoveState();
  const remove = useDeleteState();
  const promote = useMarkStateDefault();
  const memberEditing = useSetMemberEditing();

  const [expanded, setExpanded] = React.useState(initialExpanded);
  const [creatingIn, setCreatingIn] = React.useState<WorkflowGroup | null>(null);
  const [editingId, setEditingId] = React.useState<string | null>(null);
  const [deleting, setDeleting] = React.useState<State | null>(null);
  // One drag lives across the whole tab: the source may sit in one card
  // while the pointer hovers another.
  const [dragSource, setDragSource] = React.useState<string | null>(null);
  const [dropTarget, setDropTarget] = React.useState<StateDropTarget | null>(null);

  const groups = React.useMemo<GroupedStates[]>(() => groupStates(states.data ?? []), [states.data]);

  function toggleGroup(group: WorkflowGroup): void {
    setExpanded((previous) => ({ ...previous, [group]: !previous[group] }));
  }

  function openCreate(group: WorkflowGroup): void {
    setExpanded((previous) => ({ ...previous, [group]: true }));
    setCreatingIn(group);
  }

  function submitCreate(group: WorkflowGroup, value: StateFormValue): void {
    create.mutate(
      {
        workspaceSlug,
        projectId,
        group,
        input: { name: value.name, description: value.description, color: value.color },
      },
      { onSuccess: () => setCreatingIn(null) }
    );
  }

  function submitEdit(state: State, value: StateFormValue): void {
    const input = { name: value.name, description: value.description, color: value.color };
    update.mutate(
      { workspaceSlug, projectId, stateId: state.id, patch: input, input },
      { onSuccess: () => setEditingId(null) }
    );
  }

  function promoteState(state: State): void {
    promote.mutate({ workspaceSlug, projectId, stateId: state.id });
  }

  function startDrag(stateId: string): void {
    setDragSource(stateId);
    setDropTarget(null);
  }

  function endDrag(): void {
    setDragSource(null);
    setDropTarget(null);
  }

  function hoverEdge(targetId: string, edge: "above" | "below"): void {
    if (dragSource === null) return;
    // A row never marks itself as its own destination.
    if (dragSource === targetId) {
      setDropTarget(null);
      return;
    }
    setDropTarget({ id: targetId, edge });
  }

  function leaveRow(targetId: string): void {
    setDropTarget((previous) => {
      if (previous === null) return null;
      return previous.id === targetId ? null : previous;
    });
  }

  const header = (
    <div className="flex flex-col gap-(--space-4)">
      {showToggle ? (
        <MemberEditingToggle
          enabled={flag}
          pending={memberEditing.isPending}
          onChange={(enabled) => memberEditing.mutate({ workspaceSlug, projectId, enabled })}
        />
      ) : null}
    </div>
  );

  if (states.isPending) {
    return (
      <section aria-label="Workflow states" data-testid="states-tab" className="flex flex-col gap-(--space-4)">
        {header}
        <div data-testid="states-skeleton" className="flex flex-col gap-(--space-2)">
          {Array.from({ length: SKELETON_ROWS }, (_, index) => (
            <Skeleton key={index} height="48px" />
          ))}
        </div>
      </section>
    );
  }

  if (states.isError) {
    return (
      <section aria-label="Workflow states" data-testid="states-tab" className="flex flex-col gap-(--space-4)">
        {header}
        <div data-testid="states-load-failed" className="flex flex-col items-start gap-(--space-2)">
          <p className="text-body text-(--text-muted)">The workflow states couldn’t be loaded.</p>
          <Button variant="secondary" onClick={() => void states.refetch()}>
            <RefreshCw size={14} aria-hidden="true" />
            Try again
          </Button>
        </div>
      </section>
    );
  }

  // The read has landed by here, so its rows are the move resolver's input.
  const rows = states.data;
  function submitMove(sourceId: string, targetId: string, edge: "above" | "below"): void {
    const resolved = resolveStateMove(rows, sourceId, targetId, edge);
    // Dropping a row where it already sits is not a write.
    if (resolved === null) return;
    move.mutate({ workspaceSlug, projectId, move: resolved });
  }

  function dropRow(targetId: string, edge: "above" | "below"): void {
    if (dragSource !== null && dragSource !== targetId) submitMove(dragSource, targetId, edge);
    endDrag();
  }

  return (
    <section aria-label="Workflow states" data-testid="states-tab" className="flex flex-col gap-(--space-4)">
      {header}
      <div className="flex flex-col gap-(--space-3)">
        {groups.map((group) => (
          <StateGroupCard
            key={group.group}
            group={group.group}
            label={group.label}
            rows={group.rows}
            expanded={expanded[group.group]}
            onToggle={() => toggleGroup(group.group)}
            editable={editable}
            creating={creatingIn === group.group}
            onOpenCreate={() => openCreate(group.group)}
            onCancelCreate={() => setCreatingIn(null)}
            onCreate={(value) => submitCreate(group.group, value)}
            createPending={create.isPending}
            editingId={editingId}
            onEdit={setEditingId}
            onCancelEdit={() => setEditingId(null)}
            onSaveEdit={submitEdit}
            editPending={update.isPending}
            deleteBlockedReason={deleteBlockedReason}
            onDelete={setDeleting}
            onPromote={promoteState}
            promotingId={promote.isPending ? promote.variables.stateId : null}
            dragSource={dragSource}
            dropTarget={dropTarget}
            onDragStart={startDrag}
            onDragEnd={endDrag}
            onDragOver={hoverEdge}
            onDragLeave={leaveRow}
            onDrop={dropRow}
          />
        ))}
      </div>
      {deleting === null ? null : (
        <StateDeleteDialog
          stateName={deleting.name}
          pending={remove.isPending}
          onCancel={() => setDeleting(null)}
          onConfirm={() =>
            remove.mutate({ workspaceSlug, projectId, stateId: deleting.id }, { onSuccess: () => setDeleting(null) })
          }
        />
      )}
    </section>
  );
}

/** Re-exported for the route and tests: the keys this tab renders. */
export { isWorkflowGroup };
