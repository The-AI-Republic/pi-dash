// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board header add control (parity/specs/issues.md §5, ISS-036). The
// column's "+" opens the create modal prefilled with the column payload;
// in cycle or module context it offers creating a new item or adding an
// existing one instead, the same two entries the list headers use. The
// control disables where creation is off (creator groupings, read-only
// boards) rather than hiding, so its absence never shifts the header.

import { Menu } from "@pidash/kit";
import { Plus } from "lucide-react";
import * as React from "react";

export interface KanbanHeaderCreateProps {
  columnTitle: string;
  /** Cycle/module contexts offer the two-entry menu. */
  menu: boolean;
  disabled: boolean;
  /** Tooltip while disabled. */
  disabledReason?: string | undefined;
  onCreate: () => void;
  onAddExisting: () => void;
}

const TRIGGER_CLASS =
  "inline-flex size-5 cursor-pointer items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)";

export function KanbanHeaderCreate({
  columnTitle,
  menu,
  disabled,
  disabledReason,
  onCreate,
  onAddExisting,
}: KanbanHeaderCreateProps): React.ReactElement {
  if (disabled) {
    return (
      <span
        data-testid="kanban-header-create"
        role="button"
        tabIndex={-1}
        aria-label={`Add to ${columnTitle}`}
        aria-disabled={true}
        title={disabledReason}
        className="inline-flex size-5 cursor-not-allowed items-center justify-center rounded-(--radius-control) text-(--text-muted) opacity-50"
      >
        <Plus size={14} strokeWidth={2} aria-hidden="true" />
      </span>
    );
  }
  if (!menu) {
    return (
      <span
        data-testid="kanban-header-create"
        role="button"
        tabIndex={0}
        aria-label={`Add to ${columnTitle}`}
        onClick={(event) => {
          event.stopPropagation();
          onCreate();
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.stopPropagation();
            event.preventDefault();
            onCreate();
          }
        }}
        className={TRIGGER_CLASS}
      >
        <Plus size={14} strokeWidth={2} aria-hidden="true" />
      </span>
    );
  }
  return (
    <Menu
      trigger={
        <span
          data-testid="kanban-header-create"
          role="button"
          tabIndex={0}
          aria-label={`Add to ${columnTitle}`}
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") event.stopPropagation();
          }}
          className={TRIGGER_CLASS}
        >
          <Plus size={14} strokeWidth={2} aria-hidden="true" />
        </span>
      }
      items={[
        { label: "Create work item", onSelect: onCreate },
        { label: "Add an existing work item", onSelect: onAddExisting },
      ]}
    />
  );
}
