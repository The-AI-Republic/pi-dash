// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero progress card (parity/specs/cycles.md §15, CYC-041): the
// covered/total closed summary, the cancelled line, and the five
// lifecycle groups in order. Nonempty groups are buttons into the
// filtered detail; zeros read as plain rows; a cycle without work
// shows the card's empty view instead.

import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { heroProgressQueryOptions } from "../api/heroQueries.js";
import {
  heroCancelledLine,
  heroClosedSummary,
  heroProgressGroups,
  type HeroProgressGroupId,
} from "../lib/cycleHero.js";

/** Card heading. */
export const HERO_PROGRESS_TITLE = "Progress";
/** Empty view when the cycle holds no work (sic, pinned by the oracle). */
export const HERO_PROGRESS_EMPTY = "Add work items to the cycle to view it's progress";

export interface HeroProgressCardProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  onGroupClick: (group: HeroProgressGroupId) => void;
  /** True while a breakdown click is still writing. */
  disabled?: boolean;
}

export function HeroProgressCard({
  workspaceSlug,
  projectId,
  cycleId,
  onGroupClick,
  disabled = false,
}: HeroProgressCardProps): React.ReactElement {
  const progress = useQuery(heroProgressQueryOptions(getAppClient(), workspaceSlug, projectId, cycleId));
  const data = progress.data;
  let body: React.ReactNode;
  if (progress.isPending) {
    body = (
      <>
        <h3 className="text-emphasis text-(--text)">{HERO_PROGRESS_TITLE}</h3>
        <p data-testid="hero-progress-loading" className="text-meta text-(--text-muted)">
          Loading progress…
        </p>
      </>
    );
  } else if (progress.isError || data === undefined || data.total_issues === 0) {
    body = (
      <div className="text-center">
        <h3 className="text-body text-(--text-muted)">{HERO_PROGRESS_EMPTY}</h3>
      </div>
    );
  } else {
    body = (
      <>
        <h3 className="text-emphasis text-(--text)">{HERO_PROGRESS_TITLE}</h3>
        <div className="flex flex-col gap-(--space-1)">
          <span data-testid="hero-progress-closed" className="text-body text-(--text)">
            {heroClosedSummary(data)}
          </span>
          <span className="text-meta text-(--text-muted)">{heroCancelledLine(data.cancelled_issues)}</span>
        </div>
        <div className="flex flex-col">
          {heroProgressGroups(data).map((group) =>
            group.count > 0 ? (
              <button
                key={group.id}
                type="button"
                data-testid="hero-progress-group"
                data-group={group.id}
                disabled={disabled}
                onClick={() => onGroupClick(group.id)}
                className="text-body flex cursor-pointer items-center justify-between gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle) disabled:cursor-wait disabled:opacity-60"
              >
                <span className="capitalize">{group.id}</span>
                <span className="text-(--text-muted)">{group.count}</span>
              </button>
            ) : (
              <div
                key={group.id}
                data-testid="hero-progress-group"
                data-group={group.id}
                aria-disabled="true"
                className="text-body flex items-center justify-between gap-(--space-2) px-(--space-2) py-(--space-1) text-(--text-muted)"
              >
                <span className="capitalize">{group.id}</span>
                <span>0</span>
              </div>
            )
          )}
        </div>
      </>
    );
  }
  return (
    <section
      data-testid="hero-progress-card"
      aria-label={HERO_PROGRESS_TITLE}
      className="flex flex-col gap-(--space-3) rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)"
    >
      {body}
    </section>
  );
}
