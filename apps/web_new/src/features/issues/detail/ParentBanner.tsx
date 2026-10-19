// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parent banner (parity/specs/issues.md §13, ISS-158). A thin strip above
// the main column names the item's parent: state dot, identifier and
// name, with the whole strip linking to the parent. The ellipsis opens a
// menu that loads the parent's children on first open, excludes the item
// itself, lists each sibling as a link to its own page, and offers the
// parent removal that clears `parent_id`. Nothing renders without a
// parent, and the parent's own project supplies the identifier.

import { IconButton, Menu, Skeleton, type MenuEntry } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import { MoreHorizontal } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { pickerProjectQueryOptions, pickerStatesQueryOptions } from "../../../shared/pickers/index.js";
import { parentIssueQueryOptions, parentSiblingsQueryOptions, siblingRows, useParentWrite } from "../api/parent.js";
import type { Issue } from "@pidash/api-client";

/** Sibling menu copy. */
export const SIBLINGS_LOADING_TEXT = "Loading sibling work items";
export const SIBLINGS_EMPTY_TEXT = "No sibling work items";
export const REMOVE_PARENT_TEXT = "Remove parent work item";
export const PARENT_BANNER_LABEL = "Parent work item";

export interface ParentBannerProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  /** Project member/admin on a live project; guests read. */
  editable: boolean;
}

/** The parent's workflow state colour, name and group. */
export function useParentState(
  workspaceSlug: string,
  parentProjectId: string | null,
  stateId: string | null | undefined
): { color: string | null; name: string | null; group: string | null } {
  const states = useQuery({
    ...pickerStatesQueryOptions(getAppClient(), workspaceSlug, parentProjectId ?? ""),
    enabled: parentProjectId !== null && stateId !== null && stateId !== undefined,
  });
  const match = states.data?.find((state) => state.id === stateId);
  return { color: match?.color ?? null, name: match?.name ?? null, group: match?.group ?? null };
}

export function ParentBanner({
  workspaceSlug,
  projectId,
  issue,
  editable,
}: ParentBannerProps): React.ReactElement | null {
  const readOnly = !editable || issue.archived_at !== null || issue.is_synced;
  const parentId = issue.parent_id;
  const parent = useQuery(parentIssueQueryOptions(getAppClient(), workspaceSlug, projectId, parentId ?? ""));
  const parentProjectId = parent.data?.project_id ?? null;
  const project = useQuery({
    ...pickerProjectQueryOptions(getAppClient(), workspaceSlug, parentProjectId ?? ""),
    enabled: parentProjectId !== null,
  });
  const state = useParentState(workspaceSlug, parentProjectId, parent.data?.state_id ?? null);
  const [menuOpen, setMenuOpen] = React.useState(false);
  const siblings = useQuery({
    ...parentSiblingsQueryOptions(getAppClient(), workspaceSlug, parentProjectId ?? projectId, parentId ?? ""),
    enabled: parentId !== null && menuOpen && parentProjectId !== null,
  });
  const { setParent } = useParentWrite({ workspaceSlug, projectId, issueId: issue.id });

  if (parentId === null) return null;

  const identifier =
    parent.data === undefined || project.data === undefined
      ? null
      : `${project.data.identifier}-${parent.data.sequence_id}`;
  const href = identifier === null ? null : `/${workspaceSlug}/browse/${identifier}`;
  const rows = siblingRows(siblings.data?.sub_issues, issue.id);

  const items: MenuEntry[] = [];
  if (siblings.isPending) {
    items.push({
      custom: true,
      entryKey: "loading",
      node: (
        <p role="status" className="text-meta px-(--space-4) py-(--space-2) text-(--text-muted)">
          {SIBLINGS_LOADING_TEXT}
        </p>
      ),
    });
  } else if (rows.length === 0) {
    items.push({
      custom: true,
      entryKey: "empty",
      node: (
        <p role="status" className="text-meta px-(--space-4) py-(--space-2) text-(--text-muted)">
          {SIBLINGS_EMPTY_TEXT}
        </p>
      ),
    });
  } else {
    for (const row of rows) {
      const rowHref = `/${workspaceSlug}/browse/${project.data?.identifier ?? ""}-${row.sequence_id}`;
      items.push({
        custom: true,
        entryKey: row.id,
        node: (
          <a
            key={row.id}
            role="menuitem"
            href={rowHref}
            onClick={() => setMenuOpen(false)}
            className="text-body flex min-w-0 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-4) py-(--space-3) text-(--text) outline-none hover:bg-(--subtle) focus-visible:bg-(--subtle)"
          >
            <span className="text-caption shrink-0 text-(--text-muted)">{row.sequence_id}</span>
            <span className="min-w-0 truncate">{row.name}</span>
          </a>
        ),
      });
    }
  }
  if (!readOnly) {
    items.push({ separator: true });
    items.push({
      label: REMOVE_PARENT_TEXT,
      onSelect: () => {
        void setParent(null).catch(() => undefined);
      },
    });
  }

  return (
    <div
      data-testid="parent-banner"
      aria-label={PARENT_BANNER_LABEL}
      className="flex min-w-0 items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-3) py-(--space-2)"
    >
      {identifier === null || href === null || parent.data === undefined ? (
        <span className="min-w-0 flex-1">
          <Skeleton height="20px" />
        </span>
      ) : (
        <a href={href} className="group flex min-w-0 flex-1 items-center gap-(--space-2)">
          <span
            aria-hidden="true"
            title={state.name ?? undefined}
            style={state.color === null ? undefined : { backgroundColor: state.color }}
            className="size-2 shrink-0 rounded-full bg-(--text-muted)"
          />
          <span className="text-caption shrink-0 text-(--text-muted)">{identifier}</span>
          <span className="text-body min-w-0 truncate text-(--text) group-hover:underline">{parent.data.name}</span>
        </a>
      )}
      <Menu
        trigger={
          <IconButton label="Parent options" size="small">
            <MoreHorizontal size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={items}
        open={menuOpen}
        onOpenChange={setMenuOpen}
      />
    </div>
  );
}
