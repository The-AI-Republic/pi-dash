// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Active-cycle hero (parity/specs/cycles.md §15, CYC-041–042). The
// list leads with the cycle covering today: a disclosure panel with
// the active row itself plus the progress, burn-down, and breakdown
// cards. Group and entry clicks filter the cycle view through the
// shared breakdown write; the burn-down and breakdown share one
// measure. With no cycle covering today the panel becomes the empty
// view. The hero reads the unfiltered current cycle, so search and
// filters narrow the groups below without touching it.

import { useQuery } from "@tanstack/react-query";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRow } from "../../auth/index.js";
import { useApplyBreakdownFilter } from "../api/heroBreakdown.js";
import { heroQueryOptions } from "../api/heroQueries.js";
import type { BreakdownDimension } from "../lib/cycleBreakdownFilter.js";
import type { CycleMemberEntry } from "../lib/cycleMembers.js";
import type { HeroMeasure, HeroProgressGroupId } from "../lib/cycleHero.js";
import type { CycleProjectRef } from "../dialog/CycleDialog.js";
import { CycleRow } from "./CycleRow.js";
import { HeroBreakdownCard } from "./HeroBreakdownCard.js";
import { HeroBurndownCard } from "./HeroBurndownCard.js";
import { HeroProgressCard } from "./HeroProgressCard.js";
import { NoActiveCycle } from "./NoActiveCycle.js";

/** Hero disclosure heading. */
export const ACTIVE_CYCLE_HEADING = "Active cycle";

export interface ActiveCycleHeroProps {
  workspaceSlug: string;
  projectId: string;
  /** IANA name of the project's timezone. */
  projectTimeZone: string;
  /** Workspace directory behind the hero row's avatars. */
  directory: ReadonlyMap<string, CycleMemberEntry>;
  /** Members and admins mutate; guests browse the hero read-only. */
  canMutate: boolean;
  /** Open (or toggle shut) the hero row's quick look. */
  onPeek: (cycleId: string) => void;
  /** The project in the address: the empty view's creation label. */
  currentProject: CycleProjectRef;
}

export function ActiveCycleHero({
  workspaceSlug,
  projectId,
  projectTimeZone,
  directory,
  canMutate,
  onPeek,
  currentProject,
}: ActiveCycleHeroProps): React.ReactElement {
  const hero = useQuery(heroQueryOptions(getAppClient(), workspaceSlug, projectId));
  const project = useProjectRow(workspaceSlug, projectId);
  const [expanded, setExpanded] = React.useState(true);
  const [measure, setMeasure] = React.useState<HeroMeasure>("issues");
  const current = hero.data?.[0];
  const breakdown = useApplyBreakdownFilter({ workspaceSlug, projectId, cycleId: current?.id ?? "" });
  const estimatesEnabled = project.data?.estimate !== null && project.data?.estimate !== undefined;

  function onGroupClick(group: HeroProgressGroupId): void {
    void breakdown.apply("state_group", group);
  }

  function onEntryClick(dimension: BreakdownDimension, value: string, peekIssueId?: string): void {
    void breakdown.apply(dimension, value, peekIssueId);
  }

  let panel: React.ReactNode = null;
  if (expanded) {
    if (hero.isPending) {
      panel = (
        <p data-testid="cycle-hero-loading" className="text-meta px-(--space-4) text-(--text-muted)">
          Loading the active cycle…
        </p>
      );
    } else if (hero.isError) {
      panel = null;
    } else if (current === undefined) {
      panel = (
        <NoActiveCycle
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          canCreate={canMutate}
          currentProject={currentProject}
        />
      );
    } else {
      panel = (
        <>
          <CycleRow
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            cycle={current}
            projectTimeZone={projectTimeZone}
            directory={directory}
            showFavoriteMarker={canMutate}
            onPeek={onPeek}
            canMutate={canMutate}
            currentProject={currentProject}
          />
          <div data-testid="cycle-hero-cards" className="grid gap-(--space-4) px-(--space-4) lg:grid-cols-3">
            <HeroProgressCard
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              cycleId={current.id}
              onGroupClick={onGroupClick}
              disabled={breakdown.pending}
            />
            <HeroBurndownCard
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              cycleId={current.id}
              startDate={current.start_date}
              endDate={current.end_date}
              measure={measure}
              estimatesEnabled={estimatesEnabled}
              onMeasureChange={setMeasure}
            />
            <HeroBreakdownCard
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              cycleId={current.id}
              measure={measure}
              onEntryClick={onEntryClick}
              disabled={breakdown.pending}
            />
          </div>
        </>
      );
    }
  }

  return (
    <section data-testid="cycle-hero" aria-label={ACTIVE_CYCLE_HEADING} className="flex flex-col">
      <button
        type="button"
        aria-expanded={expanded}
        onClick={() => setExpanded((open) => !open)}
        className="flex cursor-pointer items-center gap-(--space-2) px-(--space-4) py-(--space-2) text-left"
      >
        <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" className={expanded ? undefined : "-rotate-90"} />
        <span className="text-emphasis text-(--text)">{ACTIVE_CYCLE_HEADING}</span>
        <span data-testid="cycle-hero-count" className="text-caption text-(--text-muted)">
          {current === undefined ? 0 : 1}
        </span>
      </button>
      {panel}
    </section>
  );
}
