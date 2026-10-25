// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One public issue in either surface (SPC-027-029, 031, 032). The list row
// keeps the key outside its title-only link; the board card makes the body
// one link and puts engagement controls beside it. Both are read-only: no
// drag handle, editable field or structural affordance exists here. The
// open action is a callback when the owner supplies one, with the address
// as the link's ordinary fallback.

import type { SpaceBoardIssue, SpaceBoardLabel, SpaceBoardState } from "@pidash/api-client";
import { Tooltip } from "@pidash/kit";
import { AlertCircle, Ban, SignalHigh, SignalLow, SignalMedium, SmilePlus, ThumbsDown, ThumbsUp } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import * as React from "react";

import { issueKey } from "../components/BoardShell.js";
import { useSpaceEmbedded } from "../lib/spaceEmbed.js";
import { StateMarker } from "./StateMarker.js";

type PriorityLevel = NonNullable<SpaceBoardIssue["priority"]>;

const PRIORITY_GLYPHS: Record<PriorityLevel, LucideIcon> = {
  urgent: AlertCircle,
  high: SignalHigh,
  medium: SignalMedium,
  low: SignalLow,
  none: Ban,
};

const PRIORITY_LABELS: Record<PriorityLevel, string> = {
  urgent: "Urgent",
  high: "High",
  medium: "Medium",
  low: "Low",
  none: "None",
};

const PRIORITY_TONES: Record<PriorityLevel, string> = {
  urgent: "text-(--danger)",
  high: "text-(--warning)",
  medium: "text-(--accent)",
  low: "text-(--text-muted)",
  none: "text-(--text-muted)",
};

const CARD_DATE_FORMAT = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  year: "numeric",
  timeZone: "UTC",
});

/** The short due-date marker the public cards use; null when unset. */
export function formatSpaceCardDate(isoDate: string | null): string | null {
  if (isoDate === null) return null;
  const parsed = new Date(`${isoDate}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime())) return null;
  return CARD_DATE_FORMAT.format(parsed);
}

/** The priority level a card renders; an unset priority is the `none` level. */
export function spaceCardPriority(priority: SpaceBoardIssue["priority"]): PriorityLevel {
  return priority ?? "none";
}

/** Labels present on the issue, in the issue's own id order. */
export function spaceCardLabels(labelIds: readonly string[], labels: readonly SpaceBoardLabel[]): SpaceBoardLabel[] {
  const byId = new Map(labels.map((label) => [label.id, label]));
  const selected: SpaceBoardLabel[] = [];
  for (const id of labelIds) {
    const label = byId.get(id);
    if (label !== undefined) selected.push(label);
  }
  return selected;
}

/** Votes split into the up/down counts the card shows. */
export function spaceCardVoteCounts(issue: Pick<SpaceBoardIssue, "vote_items">): { up: number; down: number } {
  let up = 0;
  let down = 0;
  for (const item of issue.vote_items) {
    if (item.vote === 1) up += 1;
    else if (item.vote === -1) down += 1;
  }
  return { up, down };
}

/** The character for one API reaction key; an unknown key shows the key itself. */
export function spaceCardReactionEmoji(code: string): string {
  const parts = code.split("-");
  if (parts.some((part) => !/^\d+$/.test(part))) return code;
  const points = parts.map((part) => Number(part));
  if (points.some((point) => point > 0x10ffff)) return code;
  return String.fromCodePoint(...points);
}

/** Emoji reactions grouped by their API code, preserving first-seen order. */
export function spaceCardReactionCounts(
  reactionItems: SpaceBoardIssue["reaction_items"]
): Array<{ reaction: string; emoji: string; count: number }> {
  const counts = new Map<string, number>();
  for (const item of reactionItems) counts.set(item.reaction, (counts.get(item.reaction) ?? 0) + 1);
  return [...counts].map(([reaction, count]) => ({ reaction, emoji: spaceCardReactionEmoji(reaction), count }));
}

export interface SpaceIssueRowProps {
  issue: SpaceBoardIssue;
  state: SpaceBoardState;
  labels: readonly SpaceBoardLabel[];
  projectIdentifier: string;
  /** Canonical address for this issue's open link. */
  issueHref: string;
  /** The address's open-issue key; only an exact id match highlights. */
  selected?: boolean;
  /** Rendered as a board card instead of a list row. */
  card?: boolean;
  votesEnabled?: boolean;
  reactionsEnabled?: boolean;
  /** Slice H owns the overlay; when supplied it replaces the link's default navigation. */
  onOpenIssue?: ((issue: SpaceBoardIssue) => void) | undefined;
  onVote?: ((issue: SpaceBoardIssue, vote: 1 | -1) => void) | undefined;
  onReact?: ((issue: SpaceBoardIssue, reaction: string) => void) | undefined;
}

interface MarkerStripProps {
  issue: SpaceBoardIssue;
  state: SpaceBoardState;
  labels: readonly SpaceBoardLabel[];
}

function MarkerStrip({ issue, state, labels }: MarkerStripProps): React.ReactElement {
  const priority = spaceCardPriority(issue.priority);
  const PriorityGlyph = PRIORITY_GLYPHS[priority];
  const selectedLabels = spaceCardLabels(issue.label_ids, labels);
  const dueDate = formatSpaceCardDate(issue.target_date);
  return (
    <div data-testid="space-card-markers" className="flex min-w-0 flex-wrap items-center gap-(--space-2)">
      <div
        data-testid="space-card-state"
        className="text-meta inline-flex h-5 min-w-0 items-center gap-(--space-1) text-(--text-muted)"
      >
        <StateMarker state={state} />
        <span className="truncate">{state.name}</span>
      </div>
      <div
        data-testid="space-card-priority"
        data-priority={priority}
        title={PRIORITY_LABELS[priority]}
        className={`inline-flex h-5 items-center ${PRIORITY_TONES[priority]}`}
      >
        <PriorityGlyph size={13} strokeWidth={1.75} aria-label={`Priority: ${PRIORITY_LABELS[priority]}`} />
      </div>
      {selectedLabels.length === 0 ? null : (
        <div data-testid="space-card-labels" className="flex min-w-0 flex-wrap items-center gap-(--space-1)">
          {selectedLabels.map((label) => (
            <div
              key={label.id}
              data-testid="space-card-label"
              className="text-meta inline-flex h-5 max-w-40 items-center gap-(--space-1) rounded-(--radius-full) bg-(--subtle) px-(--space-2) text-(--text)"
            >
              <span
                aria-hidden="true"
                className="size-1.5 shrink-0 rounded-full"
                style={{ backgroundColor: label.color }}
              />
              <span className="truncate">{label.name}</span>
            </div>
          ))}
        </div>
      )}
      {dueDate === null ? null : (
        <div data-testid="space-card-date" className="text-meta inline-flex h-5 items-center text-(--text-muted)">
          {dueDate}
        </div>
      )}
    </div>
  );
}

interface CardFooterProps {
  issue: SpaceBoardIssue;
  votesEnabled: boolean;
  reactionsEnabled: boolean;
  embedded: boolean;
  onVote?: ((issue: SpaceBoardIssue, vote: 1 | -1) => void) | undefined;
  onReact?: ((issue: SpaceBoardIssue, reaction: string) => void) | undefined;
}

function CardFooter({
  issue,
  votesEnabled,
  reactionsEnabled,
  embedded,
  onVote,
  onReact,
}: CardFooterProps): React.ReactElement | null {
  if (!votesEnabled && !reactionsEnabled) return null;
  const votes = spaceCardVoteCounts(issue);
  const reactions = spaceCardReactionCounts(issue.reaction_items);
  return (
    <div
      data-testid="space-board-card-footer"
      className="flex items-center gap-(--space-2) border-t border-(--border) px-(--space-2) py-(--space-1)"
    >
      {votesEnabled ? (
        <div data-testid="space-card-votes" className="flex items-center gap-(--space-1)">
          <button
            type="button"
            aria-label={`Upvote ${issue.name}`}
            aria-disabled={embedded ? "true" : undefined}
            disabled={embedded}
            data-testid="space-card-vote-up"
            onClick={embedded ? undefined : () => onVote?.(issue, 1)}
            className={`text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-control) px-(--space-1) text-(--text-muted) ${embedded ? "cursor-default opacity-60" : "cursor-pointer hover:bg-(--subtle) hover:text-(--text)"}`}
          >
            <ThumbsUp size={12} strokeWidth={1.75} aria-hidden="true" />
            <span className="tabular-nums">{votes.up}</span>
          </button>
          <button
            type="button"
            aria-label={`Downvote ${issue.name}`}
            aria-disabled={embedded ? "true" : undefined}
            disabled={embedded}
            data-testid="space-card-vote-down"
            onClick={embedded ? undefined : () => onVote?.(issue, -1)}
            className={`text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-control) px-(--space-1) text-(--text-muted) ${embedded ? "cursor-default opacity-60" : "cursor-pointer hover:bg-(--subtle) hover:text-(--text)"}`}
          >
            <ThumbsDown size={12} strokeWidth={1.75} aria-hidden="true" />
            <span className="tabular-nums">{votes.down}</span>
          </button>
        </div>
      ) : null}
      {reactionsEnabled ? (
        <div data-testid="space-card-reactions" className="flex min-w-0 items-center gap-(--space-1)">
          {reactions.map(({ reaction, emoji, count }) => (
            <button
              key={reaction}
              type="button"
              aria-label={`React with ${emoji} to ${issue.name}`}
              data-testid="space-card-reaction"
              data-reaction={reaction}
              onClick={() => onReact?.(issue, reaction)}
              className="text-meta inline-flex cursor-pointer items-center gap-(--space-1) rounded-(--radius-control) bg-(--subtle) px-(--space-1)"
            >
              <span aria-hidden="true">{emoji}</span>
              <span className="text-(--text-muted) tabular-nums">{count}</span>
            </button>
          ))}
          {embedded ? null : (
            <button
              type="button"
              aria-label={`Add a reaction to ${issue.name}`}
              aria-haspopup="dialog"
              data-testid="space-card-reaction-add"
              className="inline-flex cursor-pointer items-center rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            >
              <SmilePlus size={13} strokeWidth={1.75} aria-hidden="true" />
            </button>
          )}
        </div>
      ) : null}
    </div>
  );
}

export function SpaceIssueRow({
  issue,
  state,
  labels,
  projectIdentifier,
  issueHref,
  selected = false,
  card = false,
  votesEnabled = false,
  reactionsEnabled = false,
  onOpenIssue,
  onVote,
  onReact,
}: SpaceIssueRowProps): React.ReactElement {
  const embedded = useSpaceEmbedded();
  const [titleTooltipOpen, setTitleTooltipOpen] = React.useState(false);
  const key = issueKey(projectIdentifier, issue);
  const emphasis = selected ? "border-(--accent-strong) ring-1 ring-(--accent-strong)" : "border-(--border)";
  const showTitle = (): void => setTitleTooltipOpen(true);
  const hideTitle = (): void => setTitleTooltipOpen(false);
  const open = (event: React.MouseEvent<HTMLAnchorElement>): void => {
    if (onOpenIssue === undefined) return;
    event.preventDefault();
    onOpenIssue(issue);
  };
  if (card) {
    return (
      <div
        data-testid="space-board-card"
        data-issue-id={issue.id}
        data-state-id={state.id}
        data-selected={selected ? "true" : undefined}
        className={`group/kanban-block flex flex-col rounded-(--radius-control) border bg-(--bg) ${emphasis}`}
      >
        <Tooltip content={issue.name} open={titleTooltipOpen} onOpenChange={setTitleTooltipOpen}>
          <a
            id={`issue-${issue.id}`}
            href={issueHref}
            title={issue.name}
            aria-current={selected ? "true" : undefined}
            onClick={open}
            onFocus={showTitle}
            onBlur={hideTitle}
            className="flex cursor-pointer flex-col gap-(--space-1) px-(--space-3) py-(--space-2) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
          >
            <div className="text-meta text-(--text-muted)">{key}</div>
            <div title={issue.name} className="text-body truncate text-(--text)">
              {issue.name}
            </div>
            <MarkerStrip issue={issue} state={state} labels={labels} />
          </a>
        </Tooltip>
        <CardFooter
          issue={issue}
          votesEnabled={votesEnabled}
          reactionsEnabled={reactionsEnabled}
          embedded={embedded}
          onVote={onVote}
          onReact={onReact}
        />
      </div>
    );
  }
  return (
    <div
      data-testid="space-board-issue"
      data-issue-id={issue.id}
      data-state-id={state.id}
      data-selected={selected ? "true" : undefined}
      className={`group/list-block flex flex-col gap-(--space-1) rounded-(--radius-control) border bg-(--bg) px-(--space-3) py-(--space-2) ${emphasis}`}
    >
      <div className="flex min-w-0 items-center gap-(--space-2)">
        <div data-testid="space-issue-key" className="text-meta shrink-0 text-(--text-muted)">
          {key}
        </div>
        <Tooltip content={issue.name} open={titleTooltipOpen} onOpenChange={setTitleTooltipOpen}>
          <a
            id={`issue-${issue.id}`}
            href={issueHref}
            title={issue.name}
            aria-current={selected ? "true" : undefined}
            onClick={open}
            onFocus={showTitle}
            onBlur={hideTitle}
            className="text-body min-w-0 cursor-pointer truncate rounded-(--radius-control) text-(--text) hover:underline focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
          >
            {issue.name}
          </a>
        </Tooltip>
      </div>
      <MarkerStrip issue={issue} state={state} labels={labels} />
    </div>
  );
}
