// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One collapsible workflow-state group card (parity/specs/
// project-settings-states.md, PSET-028, PSET-029, PSET-031, PSET-035).
// The header carries the collapse chevron, the group glyph and name, and
// the add control (disabled while that group's form is open or the
// viewer may not edit). The body holds the rows, the docked create form,
// the empty note, and the drop marker that names where a dragged row
// will land. The drag itself is owned by the tab: a source dragged out
// of one card must be receivable by every other card, so the card only
// reports hover/drop events and repaints the shared source and target.

import { Button, IconButton } from "@pidash/kit";
import { ChevronDown, Plus } from "lucide-react";
import * as React from "react";

import type { State } from "@pidash/api-client";
import { canDragGroupRows, rowShare, type WorkflowGroup, WORKFLOW_GROUP_SWATCHES } from "../groups.js";
import type { DropEdge } from "../ordering.js";
import { GroupGlyph } from "./GroupGlyph.js";
import { StateForm, type StateFormValue } from "./StateForm.js";
import { StateRow } from "./StateRow.js";

export interface StateGroupCardProps {
  group: WorkflowGroup;
  label: string;
  rows: readonly State[];
  expanded: boolean;
  onToggle: () => void;
  editable: boolean;
  creating: boolean;
  onOpenCreate: () => void;
  onCancelCreate: () => void;
  onCreate: (value: StateFormValue) => void;
  createPending: boolean;
  editingId: string | null;
  onEdit: (stateId: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: (state: State, value: StateFormValue) => void;
  editPending: boolean;
  /** Delete blocked explanation for a row, or null when deletion is allowed. */
  deleteBlockedReason: (state: State, rows: readonly State[]) => string | null;
  onDelete: (state: State) => void;
  onPromote: (state: State) => void;
  promotingId: string | null;
  /** The row being dragged, owned by the tab so any card can receive it. */
  dragSource: string | null;
  /** The row and edge the pointer currently names, owned by the tab. */
  dropTarget: StateDropTarget | null;
  onDragStart: (stateId: string) => void;
  onDragEnd: () => void;
  onDragOver: (targetId: string, edge: DropEdge) => void;
  onDragLeave: (targetId: string) => void;
  onDrop: (targetId: string, edge: DropEdge) => void;
}

/** A row named by a drop, and the edge the pointer landed against. */
export interface StateDropTarget {
  id: string;
  edge: DropEdge;
}

export function StateGroupCard({
  group,
  label,
  rows,
  expanded,
  onToggle,
  editable,
  creating,
  onOpenCreate,
  onCancelCreate,
  onCreate,
  createPending,
  editingId,
  onEdit,
  onCancelEdit,
  onSaveEdit,
  editPending,
  deleteBlockedReason,
  onDelete,
  onPromote,
  promotingId,
  dragSource,
  dropTarget,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDragLeave,
  onDrop,
}: StateGroupCardProps): React.ReactElement {
  const bodyId = React.useId();
  const draggable = canDragGroupRows(rows.length, editable);

  return (
    <section
      data-testid="state-group-card"
      data-group={group}
      aria-label={`${label} states`}
      className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-3)"
    >
      <div className="flex items-center justify-between gap-(--space-2)">
        <button
          type="button"
          data-testid="state-group-toggle"
          aria-expanded={expanded}
          aria-controls={bodyId}
          onClick={onToggle}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-(--space-2) py-(--space-1) text-left"
        >
          <ChevronDown
            size={16}
            aria-hidden="true"
            className={`shrink-0 text-(--text-muted) transition-transform ${expanded ? "rotate-0" : "-rotate-90"}`}
          />
          <GroupGlyph group={group} color={WORKFLOW_GROUP_SWATCHES[group]} />
          <span className="text-body font-medium text-(--text)">{label}</span>
          <span className="text-body text-(--text-muted)">{rows.length}</span>
        </button>
        <IconButton
          data-testid="state-group-add"
          label={`Add a state to ${label}`}
          onClick={onOpenCreate}
          disabled={!editable || creating}
        >
          <Plus size={16} aria-hidden="true" />
        </IconButton>
      </div>
      {expanded ? (
        <div id={bodyId} data-testid="state-group-body" className="flex flex-col gap-(--space-2)">
          {rows.length === 0 && !creating ? (
            <div data-testid="state-group-empty" className="text-body py-(--space-4) text-center text-(--text-muted)">
              No states available for the {label} group.
              {editable ? <div>Please create a new state.</div> : null}
            </div>
          ) : null}
          {rows.map((state, index) => {
            const marker = dropTarget !== null && dropTarget.id === state.id ? dropTarget.edge : null;
            return (
              <React.Fragment key={state.id}>
                {marker === "above" ? <DropMarker /> : null}
                {editingId === state.id ? (
                  <StateForm
                    mode="edit"
                    groupLabel={label}
                    initialName={state.name}
                    initialDescription={state.description}
                    initialColor={state.color}
                    pending={editPending}
                    onSubmit={(value) => onSaveEdit(state, value)}
                    onCancel={onCancelEdit}
                  />
                ) : (
                  <StateRow
                    state={state}
                    group={group}
                    share={rowShare(index + 1, rows.length)}
                    editable={editable}
                    draggable={draggable}
                    dropEnabled={editable}
                    deleteBlockedReason={deleteBlockedReason(state, rows)}
                    onDragStart={() => onDragStart(state.id)}
                    onDragEnd={onDragEnd}
                    onDragOverEdge={(edge) => onDragOver(state.id, edge)}
                    onDragLeaveRow={() => onDragLeave(state.id)}
                    onDrop={(edge) => onDrop(state.id, edge)}
                    onEdit={() => onEdit(state.id)}
                    onPromote={() => onPromote(state)}
                    onDelete={() => onDelete(state)}
                    promoting={promotingId === state.id}
                    dragging={dragSource === state.id}
                  />
                )}
                {marker === "below" ? <DropMarker /> : null}
              </React.Fragment>
            );
          })}
          {creating ? (
            <StateForm
              mode="create"
              groupLabel={label}
              initialColor={WORKFLOW_GROUP_SWATCHES[group]}
              pending={createPending}
              onSubmit={onCreate}
              onCancel={onCancelCreate}
            />
          ) : null}
          {expanded && rows.length === 0 && editable && !creating ? (
            <Button variant="secondary" onClick={onOpenCreate}>
              Create a state
            </Button>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

function DropMarker(): React.ReactElement {
  return <div data-testid="state-drop-marker" aria-hidden="true" className="h-0.5 w-full rounded-full bg-(--accent)" />;
}
