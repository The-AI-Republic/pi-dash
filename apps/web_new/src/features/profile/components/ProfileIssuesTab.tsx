// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assigned / Created / Subscribed tab (parity/specs/profile.md §2,
// PROF-006). The tab reads one ownership-selected page, renders it in the
// stored layout (list by default, board one switch away) and lets the
// display menu change grouping, empty groups and visible columns.
// Choices live in browser-local storage scoped to the workspace and the
// viewed member, so a reload restores them. Selecting a row mounts the
// shared peek panel host with the row's title and its full-view link; the
// issues area owns the peek content behind this seam (§13).

import { useQuery } from "@tanstack/react-query";
import type { IssueListItem, ProfileOwnership } from "@pidash/api-client";
import { Button, Popover } from "@pidash/kit";
import { ChevronDown, SlidersHorizontal } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import {
  DISPLAY_SWITCH_LABELS,
  GROUP_NONE_LABEL,
  LAYOUT_LABELS,
  LayoutIcon,
  layoutDisplayMatrix,
  resolveActiveLayout,
  rowDomId,
} from "../../issues/index.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { CrossAppLink } from "../../../shared/coexistence/CrossAppLink.js";
import { PeekPanel } from "../../../shared/shell/PeekPanel.js";
import { profileIssuesQueryOptions } from "../api/queries.js";
import {
  PROFILE_TAB_LAYOUTS,
  useProfileDisplay,
  type ProfileDisplayState,
  type ProfileTabLayout,
} from "../lib/preferences.js";
import { PROFILE_GROUP_LABELS, groupIssueRows, priorityLabel, type ProfileGroupDimension } from "../lib/issueRows.js";

export interface ProfileIssuesTabProps {
  workspaceSlug: string;
  userId: string;
  ownership: ProfileOwnership;
  title: string;
}

const OFFERED: readonly ProfileTabLayout[] = PROFILE_TAB_LAYOUTS;

/** Grouping dimensions this tab offers, with the ungrouped choice first. */
const GROUP_CHOICES: readonly (ProfileGroupDimension | null)[] = [null, "priority", "state", "assignees"];

function DisplayMenu({
  state,
  onChange,
}: {
  state: ProfileDisplayState;
  onChange: (patch: Partial<ProfileDisplayState>) => void;
}): React.ReactElement {
  const switches = layoutDisplayMatrix(state.layout).switches;
  return (
    <Popover
      trigger={
        <Button variant="secondary" size="small" data-testid="profile-display-trigger">
          <SlidersHorizontal size={14} strokeWidth={2} aria-hidden="true" />
          <span>Display</span>
        </Button>
      }
      className="max-h-[24rem] w-80 overflow-y-auto p-(--space-2)"
    >
      <div
        role="group"
        aria-label="Display Properties"
        data-testid="profile-display-panel"
        className="flex flex-col gap-(--space-4)"
      >
        <section aria-label="Group by">
          <p className="text-caption mb-(--space-1) font-medium text-(--text-muted)">Group by</p>
          <div role="radiogroup" aria-label="Group by" className="flex flex-col">
            {GROUP_CHOICES.map((dimension) => {
              const selected = state.groupBy === dimension;
              return (
                <button
                  key={dimension ?? "none"}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  data-display-group={dimension ?? "none"}
                  onClick={() => {
                    onChange({ groupBy: dimension });
                  }}
                  className={`text-body flex items-center justify-between rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle) ${
                    selected ? "text-(--text)" : "text-(--text-muted)"
                  }`}
                >
                  {dimension === null ? GROUP_NONE_LABEL : PROFILE_GROUP_LABELS[dimension]}
                  {selected ? <span aria-hidden="true">✓</span> : null}
                </button>
              );
            })}
          </div>
        </section>
        <section aria-label="Options">
          <p className="text-caption mb-(--space-1) font-medium text-(--text-muted)">Options</p>
          <div className="flex flex-col">
            {switches.map((key) => {
              const on = key === "show_empty_groups" ? state.showEmptyGroups : false;
              return (
                <button
                  key={key}
                  type="button"
                  aria-pressed={on}
                  onClick={() => {
                    onChange({ showEmptyGroups: !on });
                  }}
                  className="text-body flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
                >
                  <span
                    aria-hidden="true"
                    className={`grid size-3.5 place-items-center rounded-(--radius-control) border ${
                      on ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--border)"
                    }`}
                  >
                    {on ? "✓" : null}
                  </span>
                  {DISPLAY_SWITCH_LABELS[key]}
                </button>
              );
            })}
          </div>
        </section>
      </div>
    </Popover>
  );
}

function LayoutMenu({
  active,
  onSwitch,
}: {
  active: ProfileTabLayout;
  onSwitch: (layout: ProfileTabLayout) => void;
}): React.ReactElement {
  return (
    <Popover
      trigger={
        <Button variant="secondary" size="small" aria-label={`Issue layout, current ${LAYOUT_LABELS[active]}`}>
          <LayoutIcon kind={active} />
          <span>{LAYOUT_LABELS[active]}</span>
          <ChevronDown size={12} strokeWidth={1.5} aria-hidden="true" />
        </Button>
      }
    >
      <div role="menu" aria-label="Issue layout" className="flex flex-col">
        {OFFERED.map((kind) => {
          const label = LAYOUT_LABELS[kind];
          return (
            <button
              key={kind}
              type="button"
              role="menuitemradio"
              aria-checked={kind === active}
              data-profile-layout-choice={kind}
              onClick={() => {
                if (kind !== active) onSwitch(kind);
              }}
              className="text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle)"
            >
              {label}
            </button>
          );
        })}
      </div>
    </Popover>
  );
}

function rowKey(identifier: string, issue: IssueListItem): string {
  return identifier === "" ? String(issue.sequence_id) : `${identifier}-${issue.sequence_id}`;
}

function RowButton({
  issue,
  identifier,
  onSelect,
}: {
  issue: IssueListItem;
  identifier: string;
  onSelect: () => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      id={rowDomId(issue.id)}
      data-profile-row={issue.id}
      onClick={onSelect}
      className="flex w-full items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-2) text-left hover:bg-(--subtle)"
    >
      <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
        {rowKey(identifier, issue)}
      </span>
      <span className="text-body min-w-0 flex-1 truncate text-(--text)">{issue.name}</span>
      <span className="text-caption shrink-0 text-(--text-muted)">{priorityLabel(issue.priority)}</span>
    </button>
  );
}

function isGroupDimension(value: string | null): value is ProfileGroupDimension {
  return value === "priority" || value === "state" || value === "assignees";
}

function IssuesEmpty({ showEmptyGroups }: { showEmptyGroups: boolean }): React.ReactElement {
  return (
    <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
      <h2 className="text-h3 text-(--text)">No work items</h2>
      <p className="text-body text-(--text-muted)">
        {showEmptyGroups ? "No groups carry work items yet." : "Work items in this tab will appear here."}
      </p>
    </section>
  );
}

function ListBody({
  rows,
  groupBy,
  showEmptyGroups,
  identifier,
  onSelect,
}: {
  rows: readonly IssueListItem[];
  groupBy: string | null;
  showEmptyGroups: boolean;
  identifier: (issue: IssueListItem) => string;
  onSelect: (issue: IssueListItem) => void;
}): React.ReactElement {
  const dimension = isGroupDimension(groupBy) ? groupBy : null;
  const groups = groupIssueRows(rows, dimension);
  if (groups.length === 0) return <IssuesEmpty showEmptyGroups={showEmptyGroups} />;
  return (
    <div data-testid="profile-list" className="flex flex-col">
      {groups.map((group) => (
        <section key={group.id} aria-label={group.title} data-profile-group={group.title} className="flex flex-col">
          <h3 className="text-caption bg-(--subtle) px-(--space-4) py-(--space-1) font-medium text-(--text-muted)">
            {group.title}
          </h3>
          {group.rows.map((issue) => (
            <RowButton
              key={`${group.id}-${issue.id}`}
              issue={issue}
              identifier={identifier(issue)}
              onSelect={() => {
                onSelect(issue);
              }}
            />
          ))}
        </section>
      ))}
    </div>
  );
}

function BoardBody({
  rows,
  groupBy,
  identifier,
  onSelect,
}: {
  rows: readonly IssueListItem[];
  groupBy: string | null;
  identifier: (issue: IssueListItem) => string;
  onSelect: (issue: IssueListItem) => void;
}): React.ReactElement {
  const dimension = isGroupDimension(groupBy) ? groupBy : "priority";
  const groups = groupIssueRows(rows, dimension);
  if (groups.length === 0) return <IssuesEmpty showEmptyGroups={false} />;
  return (
    <div data-testid="profile-board" className="flex gap-(--space-3) overflow-x-auto p-(--space-3)">
      {groups.map((group) => (
        <section
          key={group.id}
          aria-label={group.title}
          data-profile-board-column={group.title}
          className="flex w-64 shrink-0 flex-col gap-(--space-2) rounded-(--radius-panel) bg-(--subtle) p-(--space-2)"
        >
          <h3 className="text-caption flex items-center justify-between px-(--space-1) font-medium text-(--text-muted)">
            <span>{group.title}</span>
            <span>{group.rows.length}</span>
          </h3>
          {group.rows.map((issue) => (
            <button
              key={issue.id}
              type="button"
              id={rowDomId(issue.id)}
              data-profile-board-card={issue.id}
              onClick={() => {
                onSelect(issue);
              }}
              className="text-body flex flex-col gap-(--space-1) rounded-(--radius-panel) border border-(--border) bg-(--surface) p-(--space-2) text-left text-(--text)"
            >
              <span className="text-caption font-[family-name:var(--font-mono)] text-(--text-muted)">
                {rowKey(identifier(issue), issue)}
              </span>
              {issue.name}
            </button>
          ))}
        </section>
      ))}
    </div>
  );
}

function ProfilePeek({
  issue,
  identifier,
  workspaceSlug,
  onClose,
}: {
  issue: IssueListItem;
  identifier: string;
  workspaceSlug: string;
  onClose: () => void;
}): React.ReactElement {
  return (
    <PeekPanel
      eyebrow={identifier === "" ? undefined : rowKey(identifier, issue)}
      title={issue.name}
      titleLabel="Work item title"
      onClose={onClose}
    >
      <CrossAppLink
        to={`/${workspaceSlug}/browse/${rowKey(identifier, issue)}`}
        className="text-body text-(--accent) hover:underline"
      >
        Open full view
      </CrossAppLink>
    </PeekPanel>
  );
}

export function ProfileIssuesTab({
  workspaceSlug,
  userId,
  ownership,
  title,
}: ProfileIssuesTabProps): React.ReactElement {
  const client = getAppClient();
  const [display, updateDisplay] = useProfileDisplay(workspaceSlug, userId);
  const active = resolveActiveLayout(display.layout, OFFERED) as ProfileTabLayout;
  const projects = useQuery(projectsQueryOptions(client, workspaceSlug));
  const issues = useQuery(profileIssuesQueryOptions(client, workspaceSlug, userId, ownership, display.orderBy));
  const [peekId, setPeekId] = React.useState<string | null>(null);
  const identifiers = React.useMemo(
    () => new Map((projects.data ?? []).map((project) => [project.id, project.identifier])),
    [projects.data]
  );
  const identifier = React.useCallback(
    (issue: IssueListItem) => identifiers.get(issue.project_id) ?? "",
    [identifiers]
  );
  const rows = issues.data?.results ?? [];
  const peeked = peekId === null ? null : (rows.find((row) => row.id === peekId) ?? null);
  const switchLayout = (layout: ProfileTabLayout): void => {
    updateDisplay({ layout });
  };
  return (
    <div data-testid="profile-issues-tab" data-profile-ownership={ownership} className="flex min-h-0 flex-1 flex-col">
      <div className="hidden shrink-0 items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-2) md:flex">
        <h2 className="text-h3 text-(--text)">{title}</h2>
        <span className="text-body text-(--text-muted)">
          {issues.data?.total_count ?? 0} work item{(issues.data?.total_count ?? 0) === 1 ? "" : "s"}
        </span>
        <div className="ml-auto flex items-center gap-(--space-2)">
          <DisplayMenu state={display} onChange={updateDisplay} />
          <LayoutMenu active={active} onSwitch={switchLayout} />
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-(--space-2) border-b border-(--border) px-(--space-3) py-(--space-2) md:hidden">
        <LayoutMenu active={active} onSwitch={switchLayout} />
        <div className="ml-auto">
          <DisplayMenu state={display} onChange={updateDisplay} />
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {issues.isPending ? (
          <p className="text-body p-(--space-6) text-(--text-muted)">Loading work items…</p>
        ) : active === "kanban" ? (
          <BoardBody
            rows={rows}
            groupBy={display.groupBy}
            identifier={identifier}
            onSelect={(issue) => {
              setPeekId(issue.id);
            }}
          />
        ) : (
          <ListBody
            rows={rows}
            groupBy={display.groupBy}
            showEmptyGroups={display.showEmptyGroups}
            identifier={identifier}
            onSelect={(issue) => {
              setPeekId(issue.id);
            }}
          />
        )}
      </div>
      {peeked === null ? null : (
        <ProfilePeek
          issue={peeked}
          identifier={identifier(peeked)}
          workspaceSlug={workspaceSlug}
          onClose={() => {
            setPeekId(null);
          }}
        />
      )}
    </div>
  );
}
