// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One sub-issue row (parity/specs/issues.md §15, ISS-174/176/177/238).
// The chevron expands the row's own children in place, one level at a
// time and only on request; the identifier and name navigate to the
// child; every visible property is its own inline control writing one
// field; the ellipsis menu copies the link or, for members, removes the
// child or deletes it behind a confirm. A row that just wrote shows its
// busy state until the write settles.

import { Avatar, IconButton, Menu, type MenuEntry, Spinner } from "@pidash/kit";
import type { SubIssueItem } from "@pidash/api-client";
import { ChevronRight, Copy, MoreHorizontal, Trash2, Unlink } from "lucide-react";
import * as React from "react";

import { useSubIssues } from "./queries.js";
import { isPropertyVisible, isSubIssueOverdue, showsMergedDateRange, type SubIssueDisplayProperties } from "./rules.js";
import type { SubIssueMemberIdentity, SubIssueRowActions } from "./types.js";
import { SubIssuePropertyControls } from "./SubIssueProperties.js";

export interface SubIssueRowProps {
  row: SubIssueItem;
  workspaceSlug: string;
  /** Short code of this row's project, for its identifier. */
  projectIdentifier: string;
  /** Short code for any nested child, which may sit in another project. */
  projectCodeFor: (row: SubIssueItem) => string;
  /** Display names and avatars for assignee ids. */
  memberIdentities: Readonly<Record<string, SubIssueMemberIdentity>>;
  properties: SubIssueDisplayProperties;
  editable: boolean;
  depth: number;
  /** Ids of the ancestors, so a cycle in bad data cannot recurse. */
  ancestors: readonly string[];
  /** Labels follow the parent's kind (epics read as work items). */
  isEpic: boolean;
  actions: SubIssueRowActions;
  /** Opens the row; the identifier carries the row's own project code. */
  onOpen: (row: SubIssueItem, identifier: string) => void;
  menuActions: SubIssueRowMenuActions;
}

function identifierFor(row: SubIssueItem, projectIdentifier: string): string {
  return `${projectIdentifier}-${row.sequence_id}`;
}

function browseHref(workspaceSlug: string, identifier: string): string {
  return `/${workspaceSlug}/browse/${identifier}`;
}

/** A small state dot in the row's time slot, for a quick scan. */
function Assignees({
  ids,
  identities,
}: {
  ids: readonly string[];
  identities: Readonly<Record<string, SubIssueMemberIdentity>>;
}): React.ReactElement | null {
  if (ids.length === 0) return null;
  const names = ids.map((id) => identities[id]?.label ?? "Member");
  return (
    <span
      className="flex shrink-0 items-center -space-x-1"
      aria-label={names.length === 1 ? `Assigned to ${names[0]}` : `${names.length} assignees: ${names.join(", ")}`}
    >
      {ids.slice(0, 3).map((id) => (
        <Avatar
          key={id}
          name={identities[id]?.label ?? "Member"}
          {...(identities[id]?.avatarUrl === null || identities[id]?.avatarUrl === undefined
            ? {}
            : { src: identities[id]!.avatarUrl! })}
          size="small"
          className="ring-2 ring-(--bg)"
        />
      ))}
    </span>
  );
}

/**
 * Everything the row menu can do; members get the write entries. The
 * full edit modal belongs to NEWFRONT-631, so no edit entry is offered
 * here until that slice mounts it — a menu entry that only navigates
 * would be the wrong promise.
 */
export interface SubIssueRowMenuActions {
  copyLink: (row: SubIssueItem) => void;
  remove: (row: SubIssueItem) => void;
  delete: (row: SubIssueItem) => void;
}

export function rowMenuItems(
  row: SubIssueItem,
  editable: boolean,
  isEpic: boolean,
  actions: SubIssueRowMenuActions
): MenuEntry[] {
  // Guests read the rows only; the menu (link copy included) is a
  // member affordance, so there is nothing to draw for them.
  if (!editable) return [];
  const items: MenuEntry[] = [
    { label: "Copy link", onSelect: () => actions.copyLink(row), icon: <Copy size={14} aria-hidden="true" /> },
  ];
  items.push({ separator: true });
  items.push({
    label: isEpic ? "Remove work item" : "Remove sub-work item",
    onSelect: () => actions.remove(row),
    icon: <Unlink size={14} aria-hidden="true" />,
  });
  items.push({
    label: "Delete",
    onSelect: () => actions.delete(row),
    icon: <Trash2 size={14} aria-hidden="true" />,
  });
  return items;
}

/**
 * One nested row plus its children. Children mount only once the parent
 * row expands, and each nested row keeps its own expansion, so a deep
 * tree fetches nothing until the reader asks for it (ISS-174/238).
 */
export function SubIssueRow(props: SubIssueRowProps): React.ReactElement {
  const { row, workspaceSlug, projectIdentifier, properties, editable, depth, ancestors, isEpic } = props;
  const [expanded, setExpanded] = React.useState(false);
  const [menuOpen, setMenuOpen] = React.useState(false);
  const identifier = identifierFor(row, projectIdentifier);
  const href = browseHref(workspaceSlug, identifier);
  const repeated = ancestors.includes(row.id);
  const hasChildren = (row.sub_issues_count ?? 0) > 0 && !repeated;
  const overdue = isSubIssueOverdue(row, new Date());
  const merged = showsMergedDateRange(row, properties);
  const busy = props.actions.isBusy(row.id);
  const paddingLeft = 12 + depth * 20;

  function toggleRow(): void {
    if (!hasChildren) return;
    setExpanded((current) => !current);
  }

  const items = rowMenuItems(row, props.editable, isEpic, props.menuActions);

  return (
    <div data-sub-issue-row={identifier} className="flex flex-col">
      <div
        className="group relative flex min-h-11 items-center gap-(--space-2) rounded-(--radius-control) py-(--space-1) pr-(--space-2) hover:bg-(--subtle)"
        style={{ paddingLeft }}
        data-busy={busy ? "true" : undefined}
      >
        <span className="flex size-5 shrink-0 items-center justify-center">
          {hasChildren ? (
            <IconButton
              label={`${expanded ? "Collapse" : "Expand"} children of ${identifier}`}
              size="small"
              variant="ghost"
              aria-expanded={expanded}
              onClick={toggleRow}
            >
              <ChevronRight
                size={14}
                strokeWidth={2}
                aria-hidden="true"
                className={expanded ? "rotate-90 transition-transform" : "transition-transform"}
              />
            </IconButton>
          ) : null}
        </span>
        <div className="flex min-w-0 flex-1 items-center gap-(--space-2)">
          {busy ? <Spinner size="small" label="Saving sub-work item" /> : null}
          <a
            href={href}
            className="flex min-w-0 flex-1 items-center gap-(--space-2) outline-none"
            onClick={(event) => {
              if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
              event.preventDefault();
              props.onOpen(row, identifier);
            }}
          >
            {isPropertyVisible(properties, "key") ? (
              <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
                {identifier}
              </span>
            ) : null}
            <span className="text-body min-w-0 flex-1 truncate text-(--text)">{row.name}</span>
          </a>
          <div className="flex shrink-0 items-center gap-(--space-1)">
            {isPropertyVisible(properties, "assignee") ? (
              <Assignees ids={row.assignee_ids} identities={props.memberIdentities} />
            ) : null}
            {isPropertyVisible(properties, "state") ||
            isPropertyVisible(properties, "priority") ||
            isPropertyVisible(properties, "start_date") ||
            isPropertyVisible(properties, "due_date") ? (
              <SubIssuePropertyControls
                workspaceSlug={workspaceSlug}
                projectId={row.project_id}
                row={row}
                properties={properties}
                editable={editable}
                merged={merged}
                overdue={overdue}
                actions={props.actions}
              />
            ) : null}
          </div>
          {items.length === 0 ? null : (
            <Menu
              open={menuOpen}
              onOpenChange={setMenuOpen}
              trigger={
                <IconButton
                  label={`Actions for ${identifier}`}
                  size="small"
                  variant="ghost"
                  data-testid="sub-issue-row-menu"
                >
                  <MoreHorizontal size={14} strokeWidth={2} aria-hidden="true" />
                </IconButton>
              }
              items={items}
            />
          )}
        </div>
      </div>
      {repeated ? (
        <p className="text-caption px-(--space-3) py-(--space-1) text-(--text-muted)">This branch repeats itself.</p>
      ) : null}
      {expanded ? (
        <SubIssueChildren
          parent={row}
          workspaceSlug={workspaceSlug}
          projectIdentifier={projectIdentifier}
          projectCodeFor={props.projectCodeFor}
          memberIdentities={props.memberIdentities}
          properties={properties}
          editable={editable}
          depth={depth + 1}
          ancestors={ancestors}
          isEpic={isEpic}
          actions={props.actions}
          onOpen={props.onOpen}
          menuActions={props.menuActions}
        />
      ) : null}
    </div>
  );
}

/** A nested parent's children, fetched on first expansion. */
function SubIssueChildren(props: Omit<SubIssueRowProps, "row"> & { parent: SubIssueItem }): React.ReactElement {
  const { parent, ancestors } = props;
  const query = useSubIssues(props.workspaceSlug, parent.project_id, parent.id, { isEpic: props.isEpic });
  const rows = query.data?.pages.flatMap((page) => page.sub_issues) ?? [];
  const cycle = ancestors.includes(parent.id);
  if (cycle) {
    return (
      <p className="text-caption px-(--space-3) py-(--space-1) text-(--text-muted)">This branch repeats itself.</p>
    );
  }
  return (
    <div className="flex flex-col">
      {query.isPending ? (
        <p role="status" className="text-caption px-(--space-3) py-(--space-1) text-(--text-muted)">
          Loading children
        </p>
      ) : query.isError ? (
        <div className="flex items-center gap-(--space-2) px-(--space-3) py-(--space-1)">
          <span className="text-caption text-(--danger)">Couldn't load the children.</span>
          <button
            type="button"
            className="text-caption font-medium text-(--accent)"
            onClick={() => void query.refetch()}
          >
            Try again
          </button>
        </div>
      ) : rows.length === 0 ? (
        <p className="text-caption px-(--space-3) py-(--space-1) text-(--text-muted)">No children yet.</p>
      ) : (
        rows.map((row) => (
          <SubIssueRow
            key={row.id}
            row={row}
            workspaceSlug={props.workspaceSlug}
            projectIdentifier={props.projectCodeFor(row)}
            projectCodeFor={props.projectCodeFor}
            memberIdentities={props.memberIdentities}
            properties={props.properties}
            editable={props.editable}
            depth={props.depth}
            ancestors={[...ancestors, parent.id]}
            isEpic={props.isEpic}
            actions={props.actions}
            onOpen={props.onOpen}
            menuActions={props.menuActions}
          />
        ))
      )}
      {query.hasNextPage ? (
        <button
          type="button"
          className="text-caption w-full px-(--space-3) py-(--space-1) text-left font-medium text-(--accent)"
          disabled={query.isFetchingNextPage}
          onClick={() => void query.fetchNextPage()}
        >
          {query.isFetchingNextPage ? "Loading more…" : "Load more"}
        </button>
      ) : null}
    </div>
  );
}
