// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One workflow-state row (parity/specs/project-settings-states.md,
// PSET-028, PSET-030–033). The row shows the state glyph with its share
// ring, name and description. Editors get a hover grip, an edit control,
// a promote-to-default action (replaced by a static marker on the
// current default) and a delete control that explains when it is
// disabled. Placement during drag is the card's business: the row only
// reports enter/leave so the card can light an edge marker.

import { IconButton } from "@pidash/kit";
import { GripVertical, Pencil, Star, Trash2 } from "lucide-react";
import * as React from "react";

import type { State } from "@pidash/api-client";
import type { WorkflowGroup } from "../groups.js";
import { GroupGlyph } from "./GroupGlyph.js";

/**
 * Which edge of a row a pointer at `pointerY` names: the half above the
 * row's middle is "above", the lower half "below". Pure so the drop
 * resolution is testable without layout.
 */
export function edgeForPointer(rowTop: number, rowHeight: number, pointerY: number): "above" | "below" {
  return pointerY < rowTop + rowHeight / 2 ? "above" : "below";
}

export interface StateRowProps {
  state: State;
  group: WorkflowGroup;
  /** Share of the group this row occupies (its group position over size). */
  share: number;
  editable: boolean;
  /** Grip is offered only on multi-row groups with editing on. */
  draggable: boolean;
  /** Delete control explanation when it is disabled; null when enabled. */
  deleteBlockedReason: string | null;
  /** Fires while dragging this row, naming the row and the hovered edge. */
  onDragStart: () => void;
  onDragEnd: () => void;
  onDragOverEdge: (edge: "above" | "below") => void;
  onDragLeaveRow: () => void;
  onDrop: (edge: "above" | "below") => void;
  onEdit: () => void;
  onPromote: () => void;
  onDelete: () => void;
  /** Promote is in flight for this row. */
  promoting: boolean;
  /** This row is the drag source. */
  dragging: boolean;
}

export function StateRow({
  state,
  group,
  share,
  editable,
  draggable,
  deleteBlockedReason,
  onDragStart,
  onDragEnd,
  onDragOverEdge,
  onDragLeaveRow,
  onDrop,
  onEdit,
  onPromote,
  onDelete,
  promoting,
  dragging,
}: StateRowProps): React.ReactElement {
  function edgeAt(event: React.DragEvent<HTMLDivElement>): "above" | "below" {
    const box = event.currentTarget.getBoundingClientRect();
    return edgeForPointer(box.top, box.height, event.clientY);
  }

  return (
    <div
      data-testid="state-row"
      data-state-name={state.name}
      data-dragging={dragging ? "true" : "false"}
      className={`group relative flex items-start justify-between gap-(--space-3) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2) transition-opacity ${
        dragging ? "opacity-50" : "opacity-100"
      }`}
      onDragOver={
        draggable
          ? (event) => {
              event.preventDefault();
              onDragOverEdge(edgeAt(event));
            }
          : undefined
      }
      onDragLeave={draggable ? onDragLeaveRow : undefined}
      onDrop={
        draggable
          ? (event) => {
              event.preventDefault();
              onDrop(edgeAt(event));
            }
          : undefined
      }
    >
      <div className="flex min-w-0 items-start gap-(--space-2)">
        {draggable ? (
          <button
            type="button"
            data-testid="state-row-grip"
            aria-label={`Reorder ${state.name}`}
            draggable
            onDragStart={(event) => {
              event.dataTransfer.effectAllowed = "move";
              event.dataTransfer.setData("text/plain", state.id);
              onDragStart();
            }}
            onDragEnd={onDragEnd}
            className="text-body mt-0.5 hidden cursor-grab items-center text-(--text-muted) group-hover:inline-flex focus-visible:inline-flex"
          >
            <GripVertical size={14} aria-hidden="true" />
          </button>
        ) : null}
        <GroupGlyph group={group} color={state.color} share={share} />
        <div className="min-w-0">
          <h4 className="text-body truncate font-medium text-(--text)">{state.name}</h4>
          {state.description !== "" ? (
            <p className="text-body truncate text-(--text-muted)">{state.description}</p>
          ) : null}
        </div>
      </div>
      {editable ? (
        <div className="flex shrink-0 items-center gap-(--space-1)">
          {state.default ? (
            <span
              data-testid="state-default-marker"
              className="text-body inline-flex items-center gap-(--space-1) text-(--text-muted)"
            >
              <Star size={14} aria-hidden="true" />
              Default
            </span>
          ) : (
            <button
              type="button"
              data-testid="state-promote"
              aria-label={`Make ${state.name} the default state`}
              disabled={promoting}
              onClick={onPromote}
              className="text-body inline-flex cursor-pointer items-center gap-(--space-1) rounded-(--radius-control) px-(--space-2) text-(--text-muted) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Star size={14} aria-hidden="true" />
              Make default
            </button>
          )}
          <IconButton label={`Edit ${state.name}`} onClick={onEdit}>
            <Pencil size={14} aria-hidden="true" />
          </IconButton>
          <IconButton
            label={
              deleteBlockedReason === null ? `Delete ${state.name}` : `Delete ${state.name} — ${deleteBlockedReason}`
            }
            onClick={onDelete}
            disabled={deleteBlockedReason !== null}
          >
            <Trash2 size={14} aria-hidden="true" />
          </IconButton>
          {deleteBlockedReason === null ? null : (
            <span className="text-body text-(--text-muted)">{deleteBlockedReason}</span>
          )}
        </div>
      ) : null}
    </div>
  );
}
