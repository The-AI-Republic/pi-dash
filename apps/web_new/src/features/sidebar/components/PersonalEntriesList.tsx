// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reorderable personal-entries list for the personalization dialog
// (SHELL-070). Each row pairs a drag handle with a visibility checkbox;
// rows drag onto each other to reorder. Dumb by design: entries arrive
// resolved and every gesture reports back through callbacks, so the
// dialog owns persistence and this list stays unit-testable with
// synthetic rows. Native checkbox and HTML5 drag and drop only: the
// reorder library is a heavy dependency the dialog chunk must not carry.
import { GripVertical } from "lucide-react";
import * as React from "react";

/** One row: identity, label and visibility. */
export interface PersonalListEntry {
  key: string;
  label: string;
  enabled: boolean;
}

export interface PersonalEntriesListProps {
  entries: PersonalListEntry[];
  disabled?: boolean;
  onToggle: (key: string, enabled: boolean) => void;
  onMove: (draggedKey: string, targetKey: string) => void;
}

export function PersonalEntriesList({
  entries,
  disabled = false,
  onToggle,
  onMove,
}: PersonalEntriesListProps): React.ReactElement {
  // The in-flight drag source. A ref, not the data transfer payload:
  // the drop target only needs the source key.
  const dragged = React.useRef<string | null>(null);
  return (
    <ul className="flex flex-col gap-(--space-1)">
      {entries.map((entry) => (
        <li
          key={entry.key}
          data-testid="personal-row"
          draggable={!disabled}
          onDragStart={() => {
            dragged.current = entry.key;
          }}
          onDragOver={(event) => {
            event.preventDefault();
          }}
          onDrop={() => {
            const source = dragged.current;
            dragged.current = null;
            if (source === null) return;
            onMove(source, entry.key);
          }}
          className="flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) hover:bg-(--subtle)"
        >
          <span aria-hidden="true" className="text-(--text-muted)">
            <GripVertical size={16} strokeWidth={1.5} />
          </span>
          <label className="text-body flex min-w-0 flex-1 cursor-pointer items-center gap-(--space-2) text-(--text)">
            <input
              type="checkbox"
              checked={entry.enabled}
              onChange={(event) => onToggle(entry.key, event.target.checked)}
              disabled={disabled}
              className="size-4 shrink-0 accent-(--accent)"
            />
            <span data-testid="personal-row-label" className="truncate">
              {entry.label}
            </span>
          </label>
        </li>
      ))}
    </ul>
  );
}
