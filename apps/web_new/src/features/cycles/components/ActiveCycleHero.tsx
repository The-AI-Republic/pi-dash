// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Active-cycle hero (parity/specs/cycles.md §15, CYC-041–042). The
// list leads with the cycle covering today: a disclosure panel with
// the active row itself plus the progress, burn-down, and breakdown
// cards. Group and entry clicks filter the cycle view through the
// shared breakdown write; the burn-down and breakdown share one
// measure. With no cycle covering today the panel becomes the empty
// view. The list's filters narrow the hero with the groups: when the
// current cycle fails the predicate the panel becomes the empty view.
// Search narrows the groups below without touching the hero.

import { useQuery } from "@tanstack/react-query";
import { selectActiveEstimate, type CycleRow as CycleRowData } from "@pidash/api-client";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRow } from "../../auth/index.js";
import { projectEstimatesQueryOptions } from "../../estimates/index.js";
import { useApplyBreakdownFilter } from "../api/heroBreakdown.js";
import { heroProgressQueryOptions, heroQueryOptions } from "../api/heroQueries.js";
import type { BreakdownDimension } from "../lib/cycleBreakdownFilter.js";
import type { CycleMemberEntry } from "../lib/cycleMembers.js";
import { heroMeasureAvailable, type HeroMeasure, type HeroProgressGroupId } from "../lib/cycleHero.js";
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
  /** List-body row predicate; a rejected hero cycle falls to the empty view. */
  cycleFilter?: ((cycle: CycleRowData) => boolean) | undefined;
}

export function ActiveCycleHero({
  workspaceSlug,
  projectId,
  projectTimeZone,
  directory,
  canMutate,
  onPeek,
  currentProject,
  cycleFilter,
}: ActiveCycleHeroProps): React.ReactElement {
  const hero = useQuery(heroQueryOptions(getAppClient(), workspaceSlug, projectId));
  const project = useProjectRow(workspaceSlug, projectId);
  const [expanded, setExpanded] = React.useState(true);
  const [measure, setMeasure] = React.useState<HeroMeasure>("issues");
  const current = hero.data?.[0];
  const shown = current !== undefined && (cycleFilter === undefined || cycleFilter(current)) ? current : undefined;
  const breakdown = useApplyBreakdownFilter({ workspaceSlug, projectId, cycleId: shown?.id ?? "" });
  const estimatesEnabled = project.data?.estimate !== null && project.data?.estimate !== undefined;
  const progress = useQuery({
    ...heroProgressQueryOptions(getAppClient(), workspaceSlug, projectId, shown?.id ?? ""),
    enabled: shown !== undefined,
  });
  const pointData = (progress.data?.total_estimate_points ?? 0) > 0;
  // The family decides whether categories hide the switch, so it is
  // needed whenever the switch could otherwise appear -- including a
  // disabled estimates setting that still carries point data.
  const estimates = useQuery({
    ...projectEstimatesQueryOptions(getAppClient(), workspaceSlug, projectId),
    enabled: estimatesEnabled || pointData,
  });
  const estimateType = selectActiveEstimate(estimates.data ?? [])?.type ?? null;
  const measureAvailable = heroMeasureAvailable({
    estimatesEnabled,
    estimateType,
    totalEstimatePoints: progress.data?.total_estimate_points,
  });
  const activeMeasure: HeroMeasure = estimatesEnabled && measureAvailable ? measure : "issues";

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
    } else if (shown === undefined) {
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
            cycle={shown}
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
              cycleId={shown.id}
              onGroupClick={onGroupClick}
              disabled={breakdown.pending}
            />
            <HeroBurndownCard
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              cycleId={shown.id}
              startDate={shown.start_date}
              endDate={shown.end_date}
              measure={activeMeasure}
              estimatesEnabled={estimatesEnabled}
              estimateType={estimateType}
              onMeasureChange={setMeasure}
            />
            <HeroBreakdownCard
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              cycleId={shown.id}
              measure={activeMeasure}
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
          {shown === undefined ? 0 : 1}
        </span>
      </button>
      {panel}
    </section>
  );
}
