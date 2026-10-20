// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members table (parity/specs/project-settings-members.md §"Member table",
// §"Search, filter and sort"; PSET-018/021). Five columns, one row per
// membership. The identity cell pairs the avatar (linked to the member's
// profile) with the full name; every header opens a sort menu with the two
// directions and a clear entry once sorted, and only the active header
// paints a direction marker.

import { Avatar, Button, Menu, cn, type MenuEntry } from "@pidash/kit";
import { ArrowDown, ArrowUp, ArrowUpDown, Check, RotateCcw } from "lucide-react";
import * as React from "react";

import {
  formatJoiningDate,
  MEMBER_COLUMNS,
  type MemberColumnKey,
  type MemberRosterRow,
  type MemberSort,
} from "./roster.js";

export interface MemberTableProps {
  workspaceSlug: string;
  rows: readonly MemberRosterRow[];
  sort: MemberSort | null;
  onSortChange: (sort: MemberSort | null) => void;
  /** Resolves a stored avatar path to a renderable URL. */
  resolveAvatar: (path: string | null) => string | null;
}

const CLEAR_SORT_LABEL = "Clear sorting";

interface SortMenuCopy {
  ascending: string;
  descending: string;
}

/** Menu wording per column: text sorts A–Z, dates oldest first, roles light to heavy. */
function sortMenuCopy(column: MemberColumnKey): SortMenuCopy {
  if (column === "joining_date") return { ascending: "Oldest first", descending: "Newest first" };
  if (column === "role") return { ascending: "Guest to Admin", descending: "Admin to Guest" };
  return { ascending: "A to Z", descending: "Z to A" };
}

function SortHeader({
  spec,
  sort,
  onSortChange,
}: {
  spec: (typeof MEMBER_COLUMNS)[number];
  sort: MemberSort | null;
  onSortChange: (sort: MemberSort | null) => void;
}): React.ReactElement {
  const direction = sort?.column === spec.key ? sort.direction : null;
  const copy = sortMenuCopy(spec.key);
  const items: MenuEntry[] = [
    {
      label: copy.ascending,
      icon: <ArrowUp size={14} strokeWidth={1.5} aria-hidden="true" />,
      content: (
        <span className="flex w-full items-center justify-between gap-(--space-3)">
          <span>{copy.ascending}</span>
          {direction === "asc" ? <Check size={14} strokeWidth={2} aria-hidden="true" /> : null}
        </span>
      ),
      onSelect: () => onSortChange({ column: spec.key, direction: "asc" }),
    },
    {
      label: copy.descending,
      icon: <ArrowDown size={14} strokeWidth={1.5} aria-hidden="true" />,
      content: (
        <span className="flex w-full items-center justify-between gap-(--space-3)">
          <span>{copy.descending}</span>
          {direction === "desc" ? <Check size={14} strokeWidth={2} aria-hidden="true" /> : null}
        </span>
      ),
      onSelect: () => onSortChange({ column: spec.key, direction: "desc" }),
    },
  ];
  if (direction !== null) {
    items.push({ separator: true });
    items.push({
      label: CLEAR_SORT_LABEL,
      icon: <RotateCcw size={14} strokeWidth={1.5} aria-hidden="true" />,
      onSelect: () => onSortChange(null),
    });
  }

  const marker = direction === "asc" ? "Ascending" : direction === "desc" ? "Descending" : "Not sorted";

  return (
    <th scope="col" aria-sort={direction === "asc" ? "ascending" : direction === "desc" ? "descending" : "none"}>
      <Menu
        trigger={
          <Button
            variant="ghost"
            size="small"
            aria-label={`Sort by ${spec.label}`}
            className={cn("-ml-(--space-2) font-medium", direction === null ? "text-(--text-muted)" : "text-(--text)")}
          >
            <span>{spec.label}</span>
            <span aria-hidden="true" className="inline-flex" data-sort-marker={marker}>
              {direction === "asc" ? (
                <ArrowUp size={12} strokeWidth={2} />
              ) : direction === "desc" ? (
                <ArrowDown size={12} strokeWidth={2} />
              ) : (
                <ArrowUpDown size={12} strokeWidth={1.5} />
              )}
            </span>
          </Button>
        }
        items={items}
      />
    </th>
  );
}

function ProfileAvatar({
  workspaceSlug,
  row,
  resolveAvatar,
}: {
  workspaceSlug: string;
  row: MemberRosterRow;
  resolveAvatar: (path: string | null) => string | null;
}): React.ReactElement {
  const src = resolveAvatar(row.avatarUrl);
  const avatarProps = { name: row.fullName, size: "small" as const, ...(src === null ? {} : { src }) };
  return (
    <a
      href={`/${workspaceSlug}/profile/${row.userId}`}
      aria-label={`Open ${row.fullName}'s profile`}
      className="inline-flex rounded-full focus-visible:ring-2 focus-visible:ring-(--accent)"
    >
      <Avatar {...avatarProps} />
    </a>
  );
}

export function MemberTable({
  workspaceSlug,
  rows,
  sort,
  onSortChange,
  resolveAvatar,
}: MemberTableProps): React.ReactElement {
  return (
    <div className="overflow-x-auto">
      <table aria-label="Members" data-testid="members-table" className="w-full border-collapse text-left">
        <thead>
          <tr className="border-b border-(--border)">
            {MEMBER_COLUMNS.map((spec) => (
              <SortHeader key={spec.key} spec={spec} sort={sort} onSortChange={onSortChange} />
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr
              key={row.membershipId}
              data-testid="member-roster-row"
              data-user-id={row.userId}
              className="border-b border-(--border) last:border-b-0"
            >
              <td className="py-(--space-2) pr-(--space-4)">
                <span className="flex items-center gap-(--space-2)">
                  <ProfileAvatar workspaceSlug={workspaceSlug} row={row} resolveAvatar={resolveAvatar} />
                  <span className="truncate">{row.fullName}</span>
                </span>
              </td>
              <td className="py-(--space-2) pr-(--space-4)">{row.displayName}</td>
              <td className="py-(--space-2) pr-(--space-4)">{row.email}</td>
              <td className="py-(--space-2) pr-(--space-4)">{row.roleLabel}</td>
              <td className="py-(--space-2) pr-(--space-4)">
                <JoiningCell row={row} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function JoiningCell({ row }: { row: MemberRosterRow }): React.ReactElement | null {
  const text = formatJoiningDate(row.joinedAt);
  return text === "" ? null : <time dateTime={row.joinedAt}>{text}</time>;
}

export { CLEAR_SORT_LABEL };
