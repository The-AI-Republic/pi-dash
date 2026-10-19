// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Roster table (parity/specs/workspace-settings.md §7/§8, WSET-008/011).
// Columns follow the spec; the five sortable headers open an
// ascending/descending menu (WSET-042, §43). Identity, suspended rows and
// hidden trailing facts live in MemberRow.

import { Button, Menu, cn, type MenuEntry } from "@pidash/kit";
import { ArrowDown, ArrowUp, ChevronsUpDown } from "lucide-react";
import * as React from "react";

import type { WorkspaceRosterMember } from "@pidash/api-client";

import { MemberRow } from "./MemberRow.js";
import {
  columnOrderLabels,
  ROSTER_COLUMNS,
  ROSTER_SORTABLE_COLUMNS,
  type RosterColumn,
  type RosterFilters,
  type RosterTableColumn,
} from "./roster.js";

export interface MembersTableProps {
  workspaceSlug: string;
  rows: readonly WorkspaceRosterMember[];
  order: RosterFilters["order"];
  onOrderChange: (order: RosterFilters["order"]) => void;
  /** Resolves a stored avatar path to a renderable URL. */
  avatarUrl: (path: string | null) => string | null;
}

function sortableSpec(column: RosterTableColumn) {
  return ROSTER_SORTABLE_COLUMNS.find((entry) => entry.key === column.key) ?? null;
}

function HeaderCell({
  column,
  order,
  onOrderChange,
}: {
  column: RosterTableColumn;
  order: RosterFilters["order"];
  onOrderChange: (order: RosterFilters["order"]) => void;
}): React.ReactElement {
  const spec = sortableSpec(column);
  if (spec === null) return <th scope="col">{column.label}</th>;

  const active = order?.column === spec.key ? order.direction : null;
  const labels = columnOrderLabels(spec);
  const items: MenuEntry[] = [
    {
      label: labels.asc,
      onSelect: () => onOrderChange({ column: spec.key as RosterColumn, direction: "asc" }),
      icon: <ArrowUp size={14} strokeWidth={1.5} aria-hidden="true" />,
    },
    {
      label: labels.desc,
      onSelect: () => onOrderChange({ column: spec.key as RosterColumn, direction: "desc" }),
      icon: <ArrowDown size={14} strokeWidth={1.5} aria-hidden="true" />,
    },
  ];
  if (active !== null) {
    items.push({ separator: true });
    items.push({ label: "Clear sorting", onSelect: () => onOrderChange(null) });
  }

  return (
    <th scope="col">
      <Menu
        trigger={
          <Button
            variant="ghost"
            size="small"
            aria-label={`Sort by ${spec.label}`}
            className={cn("-ml-(--space-2) font-medium", active === null && "text-(--text-muted)")}
          >
            {column.label}
            {active === "asc" ? (
              <ArrowUp size={12} strokeWidth={2} aria-hidden="true" />
            ) : active === "desc" ? (
              <ArrowDown size={12} strokeWidth={2} aria-hidden="true" />
            ) : (
              <ChevronsUpDown size={12} strokeWidth={1.5} aria-hidden="true" />
            )}
          </Button>
        }
        items={items}
      />
    </th>
  );
}

export function MembersTable({
  workspaceSlug,
  rows,
  order,
  onOrderChange,
  avatarUrl,
}: MembersTableProps): React.ReactElement {
  return (
    <div className="overflow-x-auto">
      <table aria-label="Members" className="w-full border-collapse text-left">
        <thead>
          <tr className="border-b border-(--border)">
            {ROSTER_COLUMNS.map((column) => (
              <HeaderCell key={column.key} column={column} order={order} onOrderChange={onOrderChange} />
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <MemberRow
              key={row.id}
              workspaceSlug={workspaceSlug}
              row={row}
              avatarUrl={avatarUrl(row.member.avatar_url)}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}
