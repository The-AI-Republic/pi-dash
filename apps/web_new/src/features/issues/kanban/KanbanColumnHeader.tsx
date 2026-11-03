// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board column header (parity/specs/issues.md §5, ISS-028/030, ISS-036).
// In flat mode the header carries the value glyph, its name, the
// column's live count, the add control and a minimize/maximize control;
// collapsed, the header folds to a narrow strip with the name set
// vertically and the count beneath it, without the add control.
// Swimlane mode draws the same header across the top row of the board
// and leaves the folding to the lane headers instead.

import { cn } from "@pidash/kit";
import { ChevronDown, ChevronRight, ListTodo } from "lucide-react";
import * as React from "react";

export const COLLAPSED_COLUMN_WIDTH = 44;

export interface KanbanColumnHeaderProps {
  name: string;
  count: number;
  collapsed: boolean;
  onToggleCollapse: () => void;
  /** Add control; hidden while the column is collapsed. */
  create?: React.ReactNode | undefined;
}

export function KanbanColumnHeader({
  name,
  count,
  collapsed,
  onToggleCollapse,
  create,
}: KanbanColumnHeaderProps): React.ReactElement {
  const Chevron = collapsed ? ChevronRight : ChevronDown;
  return (
    <div
      data-testid="kanban-column-header"
      data-collapsed={collapsed ? "true" : undefined}
      className={cn(
        "flex shrink-0 items-center gap-(--space-2) border-b border-(--border) bg-(--bg) px-(--space-3)",
        collapsed ? "h-full flex-col items-center justify-start py-(--space-3)" : "h-11"
      )}
      style={collapsed ? { width: COLLAPSED_COLUMN_WIDTH } : undefined}
    >
      <ListTodo size={14} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" aria-hidden="true" />
      <span
        className={cn(
          "text-emphasis min-w-0 text-(--text)",
          collapsed ? "whitespace-nowrap [writing-mode:vertical-rl]" : "truncate"
        )}
        title={name}
      >
        {name}
      </span>
      <span data-testid="kanban-column-count" className="text-caption shrink-0 text-(--text-muted)">
        {count}
      </span>
      {collapsed || create === undefined ? null : <span className="ml-auto shrink-0">{create}</span>}
      <button
        type="button"
        aria-label={collapsed ? `Expand ${name} column` : `Collapse ${name} column`}
        aria-expanded={!collapsed}
        onClick={onToggleCollapse}
        className={cn(
          "grid size-5 shrink-0 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)",
          collapsed ? "" : "ml-auto"
        )}
      >
        <Chevron size={14} strokeWidth={2} aria-hidden="true" />
      </button>
    </div>
  );
}
