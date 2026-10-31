// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Recent-row renderers (SHELL-016/017; written from parity/specs/shell.md
// §2 and §11). Work-item rows open the shared peek panel in place; page
// and project rows navigate client-side. Each row shows its relative
// visit age plus the context its kind carries — status, priority,
// assignees or owner, project identifiers — and missing fields omit
// their marker instead of breaking the row. The priority marker shows
// the display label, never the raw enum value.

import * as React from "react";
import { FileText, Folder, SquareKanban, User } from "lucide-react";

import { Avatar, Badge } from "@pidash/kit";
import { formatRelativeVisitTime } from "../lib/relativeTime.js";
import { priorityDisplayLabel, priorityTone } from "../lib/priority.js";
import {
  issueDetailHref,
  issueKeyLabel,
  pageHref,
  projectHref,
  type RecentIssueData,
  type RecentPageData,
  type RecentProjectData,
} from "../lib/recentRows.js";

function StatusMark({ name, color }: { name: string | null; color: string | null }): React.ReactElement | null {
  if (name === null) return null;
  return (
    <span className="text-caption inline-flex min-w-0 items-center gap-(--space-1) text-(--text-muted)">
      <span
        aria-hidden="true"
        className="size-2 shrink-0 rounded-full bg-(--text-muted)"
        style={color === null ? undefined : { backgroundColor: color }}
      />
      <span className="truncate">{name}</span>
    </span>
  );
}

function PriorityMark({ priority }: { priority: string | null }): React.ReactElement | null {
  if (priority === null) return null;
  const label = priorityDisplayLabel(priority);
  if (label === null) return null;
  // Native title tooltip: the floating kit Tooltip would pull its popup
  // runtime into the initial route chain (performance budget: initial JS
  // at most 220 KB gzipped), and the label it repeats is already visible.
  return (
    <span title={label}>
      <Badge tone={priorityTone(priority)}>{label}</Badge>
    </span>
  );
}

function AssigneeDiscs({
  names,
  count,
  one,
  many,
}: {
  names: string[];
  count: number;
  one: string;
  many: string;
}): React.ReactElement | null {
  if (count <= 0) return null;
  const shown = Math.min(count, 3);
  return (
    <span role="img" aria-label={count === 1 ? one : `${count} ${many}`} className="flex shrink-0 items-center">
      {Array.from({ length: shown }, (_, index) => {
        const name = names[index];
        return name === undefined ? (
          <span
            key={`anonymous-${index}`}
            className="inline-flex size-6 items-center justify-center rounded-(--radius-full) bg-(--subtle) text-(--text-muted) [&:not(:first-child)]:-ml-2"
          >
            <User size={12} strokeWidth={1.5} aria-hidden="true" />
          </span>
        ) : (
          <Avatar key={`${name}-${index}`} name={name} size="small" className="[&:not(:first-child)]:-ml-2" />
        );
      })}
      {count > shown ? (
        <span aria-hidden="true" className="text-caption ml-(--space-1) text-(--text-muted)">
          +{count - shown}
        </span>
      ) : null}
    </span>
  );
}

interface RowShellProps {
  href: string;
  kind: string;
  onActivate: () => void;
  icon: React.ReactNode;
  title: string;
  subline: string | null;
  context: React.ReactNode;
  visitedAt: string;
}

function RowShell({
  href,
  kind,
  onActivate,
  icon,
  title,
  subline,
  context,
  visitedAt,
}: RowShellProps): React.ReactElement {
  return (
    <a
      href={href}
      data-testid="recent-row"
      data-kind={kind}
      onClick={(event) => {
        event.preventDefault();
        onActivate();
      }}
      className="flex items-center gap-(--space-3) rounded-(--radius-control) px-(--space-2) py-(--space-2) hover:bg-(--subtle)"
    >
      <span
        aria-hidden="true"
        className="grid size-8 shrink-0 place-items-center rounded-(--radius-control) bg-(--subtle) text-(--text-muted)"
      >
        {icon}
      </span>
      <span className="min-w-0 flex-1">
        <span className="text-body block truncate text-(--text)">{title}</span>
        {subline === null ? null : <span className="text-caption block truncate text-(--text-muted)">{subline}</span>}
      </span>
      <span className="flex shrink-0 items-center gap-(--space-3)">{context}</span>
      <time dateTime={visitedAt} className="text-caption w-24 shrink-0 text-right text-(--text-muted)">
        {formatRelativeVisitTime(visitedAt)}
      </time>
    </a>
  );
}

export interface RecentIssueRowProps {
  workspaceSlug: string;
  visitedAt: string;
  issue: RecentIssueData;
  stateName: string | null;
  stateColor: string | null;
  fallbackIdentifier: string | null;
  onPreview: () => void;
}

export function RecentIssueRow({
  workspaceSlug,
  visitedAt,
  issue,
  stateName,
  stateColor,
  fallbackIdentifier,
  onPreview,
}: RecentIssueRowProps): React.ReactElement {
  return (
    <RowShell
      href={issueDetailHref(workspaceSlug, issue.projectId, issue.issueId)}
      kind="issue"
      onActivate={onPreview}
      icon={<SquareKanban size={16} strokeWidth={1.5} aria-hidden="true" />}
      title={issue.title}
      subline={issueKeyLabel(issue, fallbackIdentifier)}
      context={
        <>
          <StatusMark name={stateName} color={stateColor} />
          <PriorityMark priority={issue.priority} />
          <AssigneeDiscs names={issue.assigneeNames} count={issue.assigneeCount} one="1 assignee" many="assignees" />
        </>
      }
      visitedAt={visitedAt}
    />
  );
}

export interface RecentPageRowProps {
  workspaceSlug: string;
  visitedAt: string;
  page: RecentPageData;
  onOpen: (href: string) => void;
}

export function RecentPageRow({ workspaceSlug, visitedAt, page, onOpen }: RecentPageRowProps): React.ReactElement {
  const href = pageHref(workspaceSlug, page.projectId, page.pageId);
  return (
    <RowShell
      href={href}
      kind="page"
      onActivate={() => {
        onOpen(href);
      }}
      icon={<FileText size={16} strokeWidth={1.5} aria-hidden="true" />}
      title={page.title}
      subline={page.projectIdentifier}
      context={<AssigneeDiscs names={[]} count={page.hasOwner ? 1 : 0} one="1 owner" many="owners" />}
      visitedAt={visitedAt}
    />
  );
}

export interface RecentProjectRowProps {
  workspaceSlug: string;
  visitedAt: string;
  project: RecentProjectData;
  onOpen: (href: string) => void;
}

export function RecentProjectRow({
  workspaceSlug,
  visitedAt,
  project,
  onOpen,
}: RecentProjectRowProps): React.ReactElement {
  const href = projectHref(workspaceSlug, project.projectId);
  return (
    <RowShell
      href={href}
      kind="project"
      onActivate={() => {
        onOpen(href);
      }}
      icon={<Folder size={16} strokeWidth={1.5} aria-hidden="true" />}
      title={project.title}
      subline={project.identifier}
      context={<AssigneeDiscs names={[]} count={project.memberCount} one="1 member" many="members" />}
      visitedAt={visitedAt}
    />
  );
}
