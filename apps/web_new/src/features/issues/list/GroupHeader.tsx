// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section header (parity/specs/issues.md §2, ISS-007/008/012). Chevron,
// glyph, title, count and the add control; clicking the header toggles
// the section. In cycle/module context the add is a menu (create or
// attach existing); elsewhere it opens the create modal directly.

import { Menu, cn } from "@pidash/kit";
import { ChevronDown, ChevronRight, ListTodo, Plus } from "lucide-react";
import * as React from "react";

import type { IssueGroupSection } from "./grouping.js";

export interface GroupHeaderProps {
  section: IssueGroupSection;
  /** Group dimension; drives the glyph and the menu shape. */
  dimension: string | null;
  collapsed: boolean;
  onToggle: () => void;
  /** False for guests/creator grouping (no add control at all). */
  canAdd: boolean;
  /** Cycle/module contexts offer the two-entry menu. */
  menu: boolean;
  onCreate: () => void;
  onAddExisting: () => void;
}

function GroupGlyph({ dimension }: { dimension: string | null }): React.ReactElement {
  // One work-item glyph for every dimension: the dimension is already
  // spelled out in the header title, so a second icon set would add noise.
  void dimension;
  return <ListTodo size={14} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" aria-hidden="true" />;
}

export function GroupHeader({
  section,
  dimension,
  collapsed,
  onToggle,
  canAdd,
  menu,
  onCreate,
  onAddExisting,
}: GroupHeaderProps): React.ReactElement {
  const Chevron = collapsed ? ChevronRight : ChevronDown;
  const addControl = menu ? (
    <Menu
      trigger={
        <span
          role="button"
          tabIndex={0}
          aria-label={`Add to ${section.title}`}
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") event.stopPropagation();
          }}
          className="inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <Plus size={14} strokeWidth={2} aria-hidden="true" />
        </span>
      }
      items={[
        { label: "Create work item", onSelect: onCreate },
        { label: "Add an existing work item", onSelect: onAddExisting },
      ]}
    />
  ) : (
    <div
      role="button"
      tabIndex={0}
      aria-label={`Add to ${section.title}`}
      onClick={(event) => {
        event.stopPropagation();
        onCreate();
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.stopPropagation();
          onCreate();
        }
      }}
      className="inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
    >
      <Plus size={14} strokeWidth={2} aria-hidden="true" />
    </div>
  );

  return (
    <div
      data-testid="list-group-header"
      className={cn(
        "group/list-header flex min-h-9 items-center gap-(--space-2) border-b border-(--border) px-(--space-3)",
        collapsed ? "bg-(--surface)" : "bg-(--bg)"
      )}
      onClick={(event) => {
        // Portal menu items still bubble through React's tree. Ignore
        // events whose DOM target is outside the header itself.
        if (!event.currentTarget.contains(event.target as Node)) return;
        onToggle();
      }}
      role="button"
      tabIndex={0}
      aria-expanded={!collapsed}
      onKeyDown={(event) => {
        if (!event.currentTarget.contains(event.target as Node)) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onToggle();
        }
      }}
    >
      <Chevron size={14} strokeWidth={2} className="shrink-0 text-(--text-muted)" aria-hidden="true" />
      <GroupGlyph dimension={dimension} />
      <span className="text-emphasis min-w-0 truncate text-(--text)">{section.title}</span>
      <span className="text-caption shrink-0 text-(--text-muted)">{section.total}</span>
      {canAdd ? <span className="ml-auto shrink-0">{addControl}</span> : null}
    </div>
  );
}
