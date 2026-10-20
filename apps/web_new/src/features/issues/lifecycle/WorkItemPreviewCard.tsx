// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item preview (parity/specs/issues.md §11, ISS-133): the compact
// card a reference-opens on hover — relations, links, calendar blocks,
// timeline bars. Identifier and state share the first line, the title
// follows, and the last line carries the priority glyph beside a start
// date, a due date, or the range when both are set. A due date that has
// passed on an unfinished item is highlighted; with neither date the
// line carries only the priority. The presentation rules live in
// preview.ts so they are testable without the hover primitive.

import type { IssueListItem } from "@pidash/api-client";
import { PreviewCard } from "@pidash/kit";
import { CalendarClock, CalendarDays, CircleAlert, CircleDashed } from "lucide-react";
import * as React from "react";

import {
  PRIORITY_ICONS,
  PRIORITY_LABELS,
  STATE_GROUP_ICONS,
  type PriorityLevel,
} from "../../../shared/pickers/index.js";
import { previewDateLine, type PreviewDateLine } from "./preview.js";

export interface WorkItemPreviewCardProps {
  issue: Pick<IssueListItem, "id" | "name" | "sequence_id" | "priority" | "start_date" | "target_date" | "state_id">;
  /** Short code of the owning project, e.g. "WEB". */
  projectIdentifier: string;
  /** State row when the caller already resolved it. */
  state?: { name: string; group: string } | undefined;
  /** Today as an offset-free day, for the overdue check. */
  today?: string | undefined;
}

/** The identifier a preview card renders, e.g. "WEB-42". */
export function previewKey(projectIdentifier: string, sequenceId: number): string {
  return `${projectIdentifier}-${sequenceId}`;
}

function DateLine({ line }: { line: Exclude<PreviewDateLine, null> }): React.ReactElement {
  const glyph =
    line.kind === "range" ? (
      <CalendarDays size={12} strokeWidth={1.75} aria-hidden="true" />
    ) : line.kind === "start" ? (
      <CalendarClock size={12} strokeWidth={1.75} aria-hidden="true" />
    ) : (
      <CircleAlert size={12} strokeWidth={1.75} aria-hidden="true" />
    );
  return (
    <span
      data-testid="preview-card-dates"
      data-overdue={line.overdue ? "true" : undefined}
      className={
        line.overdue
          ? "inline-flex items-center gap-(--space-1) text-(--danger)"
          : "inline-flex items-center gap-(--space-1) text-(--text-muted)"
      }
    >
      {glyph}
      <span>{line.text}</span>
    </span>
  );
}

export function WorkItemPreviewCard({
  issue,
  projectIdentifier,
  state,
  today,
}: WorkItemPreviewCardProps): React.ReactElement {
  const PriorityGlyph = PRIORITY_ICONS[issue.priority as PriorityLevel] ?? CircleDashed;
  const StateGlyph =
    state === undefined
      ? CircleDashed
      : (STATE_GROUP_ICONS[state.group as keyof typeof STATE_GROUP_ICONS] ?? CircleDashed);
  const line = previewDateLine(
    issue.start_date ?? null,
    issue.target_date ?? null,
    state?.group ?? null,
    today ?? new Date().toISOString().slice(0, 10)
  );
  return (
    <div data-testid="work-item-preview" className="flex flex-col gap-(--space-2)">
      <div className="flex items-center justify-between gap-(--space-3)">
        <span data-testid="preview-card-key" className="text-caption font-medium text-(--text-muted)">
          {previewKey(projectIdentifier, issue.sequence_id)}
        </span>
        {state === undefined ? null : (
          <span className="text-caption inline-flex items-center gap-(--space-1) text-(--text-muted)">
            <StateGlyph size={12} strokeWidth={1.75} aria-hidden="true" />
            <span data-testid="preview-card-state">{state.name}</span>
          </span>
        )}
      </div>
      <p className="text-body wrap-break-words text-(--text)">{issue.name}</p>
      <div className="flex items-center gap-(--space-2)">
        <span
          data-testid="preview-card-priority"
          title={PRIORITY_LABELS[issue.priority as PriorityLevel] ?? issue.priority}
          className="inline-flex text-(--text-muted)"
        >
          <PriorityGlyph size={14} strokeWidth={1.75} aria-label="Priority" />
        </span>
        {line === null ? null : <DateLine line={line} />}
      </div>
    </div>
  );
}

/** The card wrapped in the hover primitive for a reference surface. */
export function WorkItemPreviewHover({
  trigger,
  ...card
}: WorkItemPreviewCardProps & {
  trigger: React.ReactElement<Record<string, unknown>>;
}): React.ReactElement {
  return (
    <PreviewCard trigger={trigger}>
      <WorkItemPreviewCard {...card} />
    </PreviewCard>
  );
}
