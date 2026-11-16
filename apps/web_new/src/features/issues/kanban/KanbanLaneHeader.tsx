// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Swimlane header (parity/specs/issues.md §5, ISS-029/031). Each lane
// opens with a sticky left header carrying a collapse chevron, the
// sub-group's glyph and name, and the cumulative count across the lane's
// columns. The chevron points down while the lane is expanded and right
// while it is folded, and clicking the header toggles the lane.

import { cn } from "@pidash/kit";
import { ChevronDown, ChevronRight, ListTodo } from "lucide-react";
import * as React from "react";

export interface KanbanLaneHeaderProps {
  name: string;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
}

export function KanbanLaneHeader({ name, count, collapsed, onToggle }: KanbanLaneHeaderProps): React.ReactElement {
  const Chevron = collapsed ? ChevronRight : ChevronDown;
  return (
    <div
      data-testid="kanban-lane-header"
      role="button"
      tabIndex={0}
      aria-expanded={!collapsed}
      aria-label={`${collapsed ? "Expand" : "Collapse"} ${name} swimlane`}
      onClick={onToggle}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onToggle();
        }
      }}
      className={cn(
        "flex h-10 cursor-pointer items-center gap-(--space-2) border-b border-(--border) px-(--space-3)",
        collapsed ? "bg-(--surface)" : "bg-(--bg)"
      )}
    >
      <Chevron size={14} strokeWidth={2} className="shrink-0 text-(--text-muted)" aria-hidden="true" />
      <ListTodo size={14} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" aria-hidden="true" />
      <span data-testid="kanban-lane-name" className="text-emphasis min-w-0 truncate text-(--text)" title={name}>
        {name}
      </span>
      <span data-testid="kanban-lane-count" className="text-caption shrink-0 text-(--text-muted)">
        {count}
      </span>
    </div>
  );
}
