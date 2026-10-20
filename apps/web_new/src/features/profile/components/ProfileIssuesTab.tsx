// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assigned / Created / Subscribed tab (parity/specs/profile.md §2,
// PROF-006). The tab follows the ownership read page by page, renders the
// stored layout (list by default, board one switch away) and lets the
// display menu change grouping, empty groups, sub-work items and visible
// columns. Choices live in browser-local storage scoped to the workspace
// and the viewed member, so a reload restores them. Grouping reads the
// projects' state vocabulary so state groups carry their real names;
// selecting a row mounts the shared peek host with the row's title and
// its full-view link — the issues area owns the peek content behind this
// seam (§13).

import { useInfiniteQuery, useQueries, useQuery } from "@tanstack/react-query";
import type { IssueListItem, ProfileOwnership, State } from "@pidash/api-client";
import { Button, Popover } from "@pidash/kit";
import { ChevronDown, SlidersHorizontal } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import {
  DISPLAY_SWITCH_LABELS,
  GROUP_NONE_LABEL,
  LAYOUT_LABELS,
  LayoutIcon,
  resolveActiveLayout,
  rowDomId,
  type DisplaySwitchKey,
} from "../../issues/index.js";
import { projectStatesQueryOptions } from "../../home/index.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { pickerWorkspaceMembersQueryOptions } from "../../../shared/pickers/index.js";
import { CrossAppLink } from "../../../shared/coexistence/CrossAppLink.js";
import { PeekPanel } from "../../../shared/shell/PeekPanel.js";
import { profileIssuesQueryOptions } from "../api/queries.js";
import {
  PROFILE_TAB_LAYOUTS,
  useProfileDisplay,
  type ProfileDisplayState,
  type ProfileTabLayout,
} from "../lib/preferences.js";
import {
  PROFILE_GROUP_LABELS,
  groupIssueRows,
  isProfileStateGroup,
  priorityLabel,
  visibleIssueRows,
  type ProfileGroupDimension,
  type ProfileStateLookup,
} from "../lib/issueRows.js";

export interface ProfileIssuesTabProps {
  workspaceSlug: string;
  userId: string;
  ownership: ProfileOwnership;
  title: string;
}

const OFFERED: readonly ProfileTabLayout[] = PROFILE_TAB_LAYOUTS;

/** Grouping dimensions this tab offers, with the ungrouped choice first. */
const GROUP_CHOICES: readonly (ProfileGroupDimension | null)[] = [null, "priority", "state", "assignees"];

/** Switches the profile record carries. */
const SWITCH_KEYS: readonly DisplaySwitchKey[] = ["show_empty_groups", "sub_issue"];

/** Columns the profile rows can show, in render order. */
const PROPERTY_KEYS: readonly { key: string; label: string }[] = [
  { key: "key", label: "ID" },
  { key: "priority", label: "Priority" },
  { key: "state", label: "State" },
  { key: "assignee", label: "Assignee" },
  { key: "start_date", label: "Start date" },
  { key: "due_date", label: "Due date" },
];

function switchValue(state: ProfileDisplayState, key: DisplaySwitchKey): boolean {
  return key === "show_empty_groups" ? state.showEmptyGroups : state.showSubIssues;
}

function switchPatch(key: DisplaySwitchKey, enabled: boolean): Partial<ProfileDisplayState> {
  return key === "show_empty_groups" ? { showEmptyGroups: enabled } : { showSubIssues: enabled };
}

function DisplayMenu({
  state,
  onChange,
}: {
  state: ProfileDisplayState;
  onChange: (patch: Partial<ProfileDisplayState>) => void;
}): React.ReactElement {
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
            {SWITCH_KEYS.map((key) => {
              const on = switchValue(state, key);
              return (
                <button
                  key={key}
                  type="button"
                  aria-pressed={on}
                  data-display-switch={key}
                  onClick={() => {
                    onChange(switchPatch(key, !on));
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
        <section aria-label="Columns">
          <p className="text-caption mb-(--space-1) font-medium text-(--text-muted)">Display Properties</p>
          <div className="flex flex-wrap gap-(--space-1)">
            {PROPERTY_KEYS.map(({ key, label }) => {
              const visible = state.properties[key] !== false;
              return (
                <button
                  key={key}
                  type="button"
                  aria-pressed={visible}
                  data-display-property={key}
                  onClick={() => {
                    onChange({ properties: { ...state.properties, [key]: !visible } });
                  }}
                  className={`text-caption rounded-(--radius-full) border px-(--space-2) py-(--space-1) ${
                    visible
                      ? "border-(--accent) bg-(--accent-subtle) text-(--text)"
                      : "border-(--border) text-(--text-muted)"
                  }`}
                >
                  {label}
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

function isGroupDimension(value: string | null): value is ProfileGroupDimension {
  return value === "priority" || value === "state" || value === "assignees";
}

interface RowContext {
  identifier: (issue: IssueListItem) => string;
  stateName: (issue: IssueListItem) => string;
  show: (key: string) => boolean;
  onSelect: (issue: IssueListItem) => void;
}

function RowButton({ issue, context }: { issue: IssueListItem; context: RowContext }): React.ReactElement {
  return (
    <button
      type="button"
      id={rowDomId(issue.id)}
      data-profile-row={issue.id}
      onClick={() => {
        context.onSelect(issue);
      }}
      className="flex w-full items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-2) text-left hover:bg-(--subtle)"
    >
      {context.show("key") ? (
        <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
          {rowKey(context.identifier(issue), issue)}
        </span>
      ) : null}
      <span className="text-body min-w-0 flex-1 truncate text-(--text)">{issue.name}</span>
      {context.show("state") ? (
        <span className="text-caption shrink-0 text-(--text-muted)">{context.stateName(issue)}</span>
      ) : null}
      {context.show("priority") ? (
        <span className="text-caption shrink-0 text-(--text-muted)">{priorityLabel(issue.priority)}</span>
      ) : null}
      {context.show("assignee") ? (
        <span className="text-caption shrink-0 text-(--text-muted)">
          {issue.assignee_ids.length === 0 ? "Unassigned" : `${String(issue.assignee_ids.length)} assigned`}
        </span>
      ) : null}
      {context.show("start_date") ? (
        <span className="text-caption shrink-0 text-(--text-muted)">{issue.start_date ?? "—"}</span>
      ) : null}
      {context.show("due_date") ? (
        <span className="text-caption shrink-0 text-(--text-muted)">{issue.target_date ?? "—"}</span>
      ) : null}
    </button>
  );
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
  memberNames,
  context,
  stateLookup,
}: {
  rows: readonly IssueListItem[];
  groupBy: string | null;
  showEmptyGroups: boolean;
  memberNames: ReadonlyMap<string, string>;
  context: RowContext;
  stateLookup: ProfileStateLookup;
}): React.ReactElement {
  const dimension = isGroupDimension(groupBy) ? groupBy : null;
  const groups = groupIssueRows(rows, dimension, stateLookup, { showEmptyGroups, memberNames });
  if (groups.length === 0) return <IssuesEmpty showEmptyGroups={showEmptyGroups} />;
  return (
    <div data-testid="profile-list" className="flex flex-col">
      {groups.map((group) => (
        <section key={group.id} aria-label={group.title} data-profile-group={group.title} className="flex flex-col">
          <h3 className="text-caption bg-(--subtle) px-(--space-4) py-(--space-1) font-medium text-(--text-muted)">
            {group.title}
          </h3>
          {group.rows.map((issue) => (
            <RowButton key={`${group.id}-${issue.id}`} issue={issue} context={context} />
          ))}
        </section>
      ))}
    </div>
  );
}

function BoardBody({
  rows,
  groupBy,
  showEmptyGroups,
  memberNames,
  context,
  stateLookup,
}: {
  rows: readonly IssueListItem[];
  groupBy: string | null;
  showEmptyGroups: boolean;
  memberNames: ReadonlyMap<string, string>;
  context: RowContext;
  stateLookup: ProfileStateLookup;
}): React.ReactElement {
  const dimension = isGroupDimension(groupBy) ? groupBy : "priority";
  const groups = groupIssueRows(rows, dimension, stateLookup, { showEmptyGroups, memberNames });
  if (groups.length === 0) return <IssuesEmpty showEmptyGroups={showEmptyGroups} />;
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
                context.onSelect(issue);
              }}
              className="text-body flex flex-col gap-(--space-1) rounded-(--radius-panel) border border-(--border) bg-(--surface) p-(--space-2) text-left text-(--text)"
            >
              <span className="text-caption font-[family-name:var(--font-mono)] text-(--text-muted)">
                {rowKey(context.identifier(issue), issue)}
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

/** The projects' states, flattened into one id → state lookup. */
function useStateLookup(workspaceSlug: string, projectIds: readonly string[]): ProfileStateLookup {
  const client = getAppClient();
  const queries = useQueries({
    queries: projectIds.map((projectId) => projectStatesQueryOptions(client, workspaceSlug, projectId)),
  });
  return React.useMemo(() => {
    const states = new Map<string, State>();
    for (const query of queries) {
      for (const state of query.data ?? []) states.set(state.id, state);
    }
    return {
      group: (id) => {
        const group = states.get(id)?.group;
        return group !== undefined && isProfileStateGroup(group) ? group : undefined;
      },
      name: (id) => states.get(id)?.name,
    };
  }, [queries]);
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
  const directory = useQuery(pickerWorkspaceMembersQueryOptions(client, workspaceSlug));
  const issues = useInfiniteQuery(profileIssuesQueryOptions(client, workspaceSlug, userId, ownership, display.orderBy));
  const [peekId, setPeekId] = React.useState<string | null>(null);
  const identifiers = React.useMemo(
    () => new Map((projects.data ?? []).map((project) => [project.id, project.identifier])),
    [projects.data]
  );
  const identifier = React.useCallback(
    (issue: IssueListItem) => identifiers.get(issue.project_id) ?? "",
    [identifiers]
  );
  const loaded = React.useMemo(() => issues.data?.pages.flatMap((page) => page.results) ?? [], [issues.data]);
  const rows = React.useMemo(() => visibleIssueRows(loaded, display.showSubIssues), [loaded, display.showSubIssues]);
  const projectIds = React.useMemo(() => [...new Set(loaded.map((row) => row.project_id))].sort(), [loaded]);
  const memberNames = React.useMemo(
    () => new Map((directory.data ?? []).map((entry) => [entry.member.id, entry.member.display_name])),
    [directory.data]
  );
  const stateLookup = useStateLookup(workspaceSlug, projectIds);
  const stateName = React.useCallback(
    (issue: IssueListItem) => (issue.state_id === null ? "" : (stateLookup.name(issue.state_id) ?? "")),
    [stateLookup]
  );
  const total = issues.data?.pages[0]?.total_count ?? 0;
  const peeked = peekId === null ? null : (loaded.find((row) => row.id === peekId) ?? null);
  const show = React.useCallback((key: string) => display.properties[key] !== false, [display.properties]);
  const context: RowContext = {
    identifier,
    stateName,
    show,
    onSelect: (issue) => {
      setPeekId(issue.id);
    },
  };
  const switchLayout = (layout: ProfileTabLayout): void => {
    updateDisplay({ layout });
  };
  return (
    <div data-testid="profile-issues-tab" data-profile-ownership={ownership} className="flex min-h-0 flex-1 flex-col">
      <div className="hidden shrink-0 items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-2) md:flex">
        <h2 className="text-h3 text-(--text)">{title}</h2>
        <span className="text-body text-(--text-muted)">
          {total} work item{total === 1 ? "" : "s"}
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
            showEmptyGroups={display.showEmptyGroups}
            memberNames={memberNames}
            context={context}
            stateLookup={stateLookup}
          />
        ) : (
          <ListBody
            rows={rows}
            groupBy={display.groupBy}
            showEmptyGroups={display.showEmptyGroups}
            memberNames={memberNames}
            context={context}
            stateLookup={stateLookup}
          />
        )}
        {issues.hasNextPage ? (
          <div className="flex justify-center p-(--space-3)">
            <Button
              variant="secondary"
              size="small"
              data-testid="profile-issues-load-more"
              disabled={issues.isFetchingNextPage}
              onClick={() => {
                void issues.fetchNextPage();
              }}
            >
              {issues.isFetchingNextPage ? "Loading…" : "Load more"}
            </Button>
          </div>
        ) : null}
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
