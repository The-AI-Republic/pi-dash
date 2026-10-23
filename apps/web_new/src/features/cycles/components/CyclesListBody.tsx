// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project cycles list body (parity/specs/cycles.md §2–§5, CYC-001–005,
// CYC-009). The hero slot leads, then the collapsible upcoming and
// completed groups with their counts; current cycles never repeat in a
// group. Rows narrow client-side by the remembered search query and the
// filter slice's predicate — no narrowing step issues a request — and
// narrowing to zero rows falls to the no-match view for the cause. The
// peeked cycle renders the quick look over the list. The screen owns
// the gate, zero, skeleton, and error states; this body only renders
// once cycles exist.

import { useQuery } from "@tanstack/react-query";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePickerWorkspaceMembers } from "../../../shared/pickers/reference.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { isProjectAuthorized } from "../../projects/index.js";
import { cyclesListQueryOptions } from "../api/queries.js";
import { useCycleSearchStore } from "../api/searchStore.js";
import { filterCyclesByQuery } from "../lib/cycleSearch.js";
import { groupOfCycle, type CycleListGroup } from "../lib/cycleGroups.js";
import { selectCycleMemberDirectory } from "../lib/cycleMembers.js";
import { CycleGroup } from "./CycleGroup.js";
import { CyclePeekPanel } from "./CyclePeekPanel.js";
import { CycleRow } from "./CycleRow.js";
import { CyclesNoMatch } from "./CyclesNoMatch.js";
import { CyclesSearchBox } from "./CyclesSearchBox.js";

export interface CyclesListBodyProps {
  workspaceSlug: string;
  projectId: string;
  /** Active-cycle hero. Slot owner: NEWFRONT-714. */
  hero?: React.ReactNode;
  /** Lifecycle/date filter control. Slot owner: NEWFRONT-709. */
  filterControl?: React.ReactNode;
  /** List header actions. Slot owner: NEWFRONT-710. */
  headerActions?: React.ReactNode;
  /** Row predicate from the filter slice. Slot owner: NEWFRONT-709. */
  cycleFilter?: ((cycle: CycleRowData) => boolean) | undefined;
  /** Clears every filter dimension. Slot owner: NEWFRONT-709. */
  onClearFilters?: (() => void) | undefined;
  /** Peeked cycle id from ?peekCycle=; null when no peek is open. */
  peekCycleId: string | null;
  onPeekChange: (cycleId: string | null) => void;
}

export function CyclesListBody({
  workspaceSlug,
  projectId,
  hero,
  filterControl,
  headerActions,
  cycleFilter,
  onClearFilters,
  peekCycleId,
  onPeekChange,
}: CyclesListBodyProps): React.ReactElement {
  const cycles = useQuery(cyclesListQueryOptions(getAppClient(), workspaceSlug, projectId));
  const project = useProjectRow(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);
  const directoryQuery = usePickerWorkspaceMembers(workspaceSlug, true);
  const query = useCycleSearchStore((state) => state.queryFor(projectId));
  const [expanded, setExpanded] = React.useState<Record<CycleListGroup, boolean>>({
    upcoming: true,
    completed: false,
  });

  const groups = React.useMemo(() => {
    const matching = filterCyclesByQuery(cycles.data ?? [], query);
    const visible = cycleFilter === undefined ? matching : matching.filter(cycleFilter);
    return {
      upcoming: visible.filter((cycle) => groupOfCycle(cycle.status) === "upcoming"),
      completed: visible.filter((cycle) => groupOfCycle(cycle.status) === "completed"),
      visible,
    };
  }, [cycles.data, query, cycleFilter]);

  const directory = React.useMemo(() => selectCycleMemberDirectory(directoryQuery.data ?? []), [directoryQuery.data]);

  const projectTimeZone = project.data?.timezone ?? "UTC";
  const showFavoriteMarker = isProjectAuthorized(roles.data?.[projectId] ?? null);
  const loaded = cycles.data ?? [];
  const noMatch = loaded.length > 0 && groups.visible.length === 0;
  const peeking = peekCycleId === null ? null : (loaded.find((cycle) => cycle.id === peekCycleId) ?? null);

  const toggleGroup = (group: CycleListGroup): void => {
    setExpanded((current) => ({ ...current, [group]: !current[group] }));
  };

  const onPeek = (cycleId: string): void => {
    onPeekChange(cycleId === peekCycleId ? null : cycleId);
  };

  const rowProps = {
    workspaceSlug,
    projectId,
    projectTimeZone,
    directory,
    showFavoriteMarker,
    onPeek,
  };

  return (
    <div data-testid="cycles-list-body" className="flex min-h-0 flex-1 flex-col gap-(--space-4)">
      <div className="flex items-center gap-(--space-2)">
        <CyclesSearchBox projectId={projectId} />
        <span className="flex-1" aria-hidden="true" />
        {filterControl}
        {headerActions}
      </div>
      {hero}
      {noMatch ? (
        <CyclesNoMatch
          variant={query === "" ? "filters" : "search"}
          onClearSearch={() => useCycleSearchStore.getState().clearQuery(projectId)}
          onClearFilters={onClearFilters}
        />
      ) : (
        <>
          <CycleGroup
            group="upcoming"
            count={groups.upcoming.length}
            expanded={expanded.upcoming}
            onToggle={() => toggleGroup("upcoming")}
          >
            {groups.upcoming.map((cycle) => (
              <CycleRow key={cycle.id} {...rowProps} cycle={cycle} />
            ))}
          </CycleGroup>
          <CycleGroup
            group="completed"
            count={groups.completed.length}
            expanded={expanded.completed}
            onToggle={() => toggleGroup("completed")}
          >
            {groups.completed.map((cycle) => (
              <CycleRow key={cycle.id} {...rowProps} cycle={cycle} />
            ))}
          </CycleGroup>
        </>
      )}
      {peekCycleId === null ? null : (
        <CyclePeekPanel
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          cycleId={peekCycleId}
          fallbackName={peeking?.name ?? ""}
          projectTimeZone={projectTimeZone}
          onClose={() => onPeekChange(null)}
        />
      )}
    </div>
  );
}
