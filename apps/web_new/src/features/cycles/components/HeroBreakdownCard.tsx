// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero breakdown card (parity/specs/cycles.md §15, CYC-041): the
// priority, assignee, and label tabs with clickable entries into the
// filtered detail. The active tab persists per cycle; every tab shows
// its own empty view.

import { useQuery } from "@tanstack/react-query";
import { Avatar } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { heroAnalyticsQueryOptions, heroIssuesQueryOptions } from "../api/heroQueries.js";
import type { BreakdownDimension } from "../lib/cycleBreakdownFilter.js";
import {
  HERO_BREAKDOWN_TABS,
  heroAssigneeEntries,
  heroLabelEntries,
  heroPriorityEntries,
  heroTabStorageKey,
  parseHeroTab,
  type HeroBreakdownTab,
  type HeroMeasure,
} from "../lib/cycleHero.js";

/** Per-tab empty views; the spec pins no copy for these. */
export const HERO_PRIORITY_EMPTY = "No urgent or high work items yet";
export const HERO_ASSIGNEES_EMPTY = "No assignees yet";
export const HERO_LABELS_EMPTY = "No labels yet";

/** The kept tab, or the priority tab where storage is missing or denied. */
export function readStoredHeroTab(cycleId: string): HeroBreakdownTab {
  try {
    return parseHeroTab(globalThis.localStorage.getItem(heroTabStorageKey(cycleId)));
  } catch {
    return "priority";
  }
}

/** Keep the tab; private modes simply stay on memory. */
export function storeHeroTab(cycleId: string, tab: HeroBreakdownTab): void {
  try {
    globalThis.localStorage.setItem(heroTabStorageKey(cycleId), tab);
  } catch {
    // Private modes deny storage; the tab still switches for the visit.
  }
}

export interface HeroBreakdownCardProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  measure: HeroMeasure;
  onEntryClick: (dimension: BreakdownDimension, value: string, peekIssueId?: string) => void;
  /** True while a breakdown click is still writing. */
  disabled?: boolean;
}

function entryClass(disabled: boolean): string {
  return `text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)${disabled ? " cursor-wait opacity-60" : ""}`;
}

export function HeroBreakdownCard({
  workspaceSlug,
  projectId,
  cycleId,
  measure,
  onEntryClick,
  disabled = false,
}: HeroBreakdownCardProps): React.ReactElement {
  const client = getAppClient();
  const [tab, setTab] = React.useState<HeroBreakdownTab>(() => readStoredHeroTab(cycleId));
  const analytics = useQuery(heroAnalyticsQueryOptions(client, workspaceSlug, projectId, cycleId, measure));
  const issues = useQuery(heroIssuesQueryOptions(client, workspaceSlug, projectId, cycleId));
  const distribution = analytics.isError ? undefined : analytics.data;

  function select(next: HeroBreakdownTab): void {
    setTab(next);
    storeHeroTab(cycleId, next);
  }

  let panel: React.ReactNode;
  if (analytics.isPending || (tab === "priority" && issues.isPending)) {
    panel = (
      <p data-testid="hero-breakdown-loading" className="text-meta p-(--space-2) text-(--text-muted)">
        Loading breakdown…
      </p>
    );
  } else if (tab === "priority") {
    const entries = issues.isError || issues.data === undefined ? [] : heroPriorityEntries(issues.data.results);
    panel =
      entries.length === 0 ? (
        <div className="text-center">
          <h3 className="text-body text-(--text-muted)">{HERO_PRIORITY_EMPTY}</h3>
        </div>
      ) : (
        <div className="flex flex-col">
          {entries.map((entry) => (
            <button
              key={entry.id}
              type="button"
              data-testid="hero-breakdown-entry"
              data-dimension="priority"
              data-value={entry.priority}
              disabled={disabled}
              onClick={() => onEntryClick("priority", entry.priority, entry.id)}
              className={entryClass(disabled)}
            >
              <span className="min-w-0 flex-1 truncate">{entry.name}</span>
              <span className="text-meta shrink-0 text-(--text-muted) capitalize">{entry.priority}</span>
            </button>
          ))}
        </div>
      );
  } else if (tab === "assignees") {
    const entries = distribution === undefined ? [] : heroAssigneeEntries(distribution, measure);
    panel =
      entries.length === 0 ? (
        <div className="text-center">
          <h3 className="text-body text-(--text-muted)">{HERO_ASSIGNEES_EMPTY}</h3>
        </div>
      ) : (
        <div className="flex flex-col">
          {entries.map((entry) => (
            <button
              key={entry.id}
              type="button"
              data-testid="hero-breakdown-entry"
              data-dimension="assignee_id"
              data-value={entry.id}
              disabled={disabled}
              onClick={() => onEntryClick("assignee_id", entry.id)}
              className={entryClass(disabled)}
            >
              <Avatar name={entry.name} size="small" {...(entry.avatar === null ? {} : { src: entry.avatar })} />
              <span className="min-w-0 flex-1 truncate">{entry.name}</span>
              <span className="text-(--text-muted)">{entry.count}</span>
            </button>
          ))}
        </div>
      );
  } else {
    const entries = distribution === undefined ? [] : heroLabelEntries(distribution, measure);
    panel =
      entries.length === 0 ? (
        <div className="text-center">
          <h3 className="text-body text-(--text-muted)">{HERO_LABELS_EMPTY}</h3>
        </div>
      ) : (
        <div className="flex flex-col">
          {entries.map((entry) => (
            <button
              key={entry.id}
              type="button"
              data-testid="hero-breakdown-entry"
              data-dimension="label_id"
              data-value={entry.id}
              disabled={disabled}
              onClick={() => onEntryClick("label_id", entry.id)}
              className={entryClass(disabled)}
            >
              <span
                aria-hidden="true"
                style={entry.color === null ? undefined : { backgroundColor: entry.color }}
                className="size-2 shrink-0 rounded-full bg-(--subtle)"
              />
              <span className="min-w-0 flex-1 truncate">{entry.name}</span>
              <span className="text-(--text-muted)">{entry.count}</span>
            </button>
          ))}
        </div>
      );
  }

  return (
    <section
      data-testid="hero-breakdown-card"
      aria-label="Breakdown"
      className="flex flex-col gap-(--space-2) rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)"
    >
      <div role="tablist" aria-label="Breakdown" className="flex items-center gap-(--space-1)">
        {HERO_BREAKDOWN_TABS.map((candidate) => (
          <button
            key={candidate.id}
            type="button"
            role="tab"
            aria-selected={tab === candidate.id}
            onClick={() => select(candidate.id)}
            className="text-meta cursor-pointer rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) data-[active=true]:bg-(--subtle) data-[active=true]:text-(--text)"
            data-active={tab === candidate.id}
          >
            {candidate.label}
          </button>
        ))}
      </div>
      <div role="tabpanel" data-tab={tab}>
        {panel}
      </div>
    </section>
  );
}
