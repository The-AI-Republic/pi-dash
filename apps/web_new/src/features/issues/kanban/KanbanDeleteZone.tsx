// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board delete zone (parity/specs/issues.md §5, ISS-040). The pill pins
// to the top center of the board and fades in while a card drag is in
// flight; it turns red under the pointer, and releasing a card over it
// hands the card to the delete confirmation instead of moving it. The
// zone stays mounted while idle so its geometry never shifts the board.

import { cn } from "@pidash/kit";
import { Trash2 } from "lucide-react";
import * as React from "react";

export interface KanbanDeleteZoneProps {
  /** True while any card drag is in flight. */
  active: boolean;
  /** True while the pointer hovers the zone mid-drag. */
  hovered: boolean;
}

export function KanbanDeleteZone({ active, hovered }: KanbanDeleteZoneProps): React.ReactElement {
  return (
    <div
      data-testid="kanban-delete-zone"
      data-active={active ? "true" : undefined}
      data-hovered={hovered ? "true" : undefined}
      aria-hidden={!active}
      className={cn(
        "pointer-events-none absolute top-(--space-2) left-1/2 z-30 -translate-x-1/2 transition-opacity",
        active ? "opacity-100" : "opacity-0"
      )}
    >
      <div
        className={cn(
          "flex items-center gap-(--space-2) rounded-full border px-(--space-4) py-(--space-2) text-(--text)",
          hovered ? "border-(--danger) bg-(--surface)" : "border-(--border) bg-(--surface)"
        )}
      >
        <Trash2
          size={14}
          strokeWidth={2}
          aria-hidden="true"
          className={hovered ? "text-(--danger)" : "text-(--text-muted)"}
        />
        <span className={cn("text-emphasis", hovered ? "text-(--danger)" : "text-(--text-muted)")}>
          Release to delete this work item
        </span>
      </div>
    </div>
  );
}
