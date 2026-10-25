// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycles tab content (parity/specs/archives.md §4,
// ARCH-014–017, ARCH-019). A plain flat list of the project's
// archived cycles — no group headings, unlike the live screen — with
// the expandable search box, the date-only filter menu and its chip
// row, the skeleton, zero, no-match and error states, and the
// address-driven read-only peek. Rows narrow client-side by the
// remembered query and the archived slot's predicate; no narrowing
// step issues a request. Row menus come from the shared cycle menu,
// so restore runs straight from the menu with the pinned stay-put
// behavior and guests keep the link actions only.

import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePickerWorkspaceMembers } from "../../../shared/pickers/reference.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { isProjectAuthorized } from "../../projects/index.js";
// Deep imports: this screen is reached from an eager route definition, so
// the cycles barrel would pin its whole export surface into the initial
// chunk (size gate).
import { archivedCyclesQueryOptions } from "../../cycles/api/queries.js";
import { useCycleSearchStore } from "../../cycles/api/searchStore.js";
import { CycleFilterChips } from "../../cycles/components/CycleFilterChips.js";
import { CycleFilterMenu } from "../../cycles/components/CycleFilterMenu.js";
import { CycleRow } from "../../cycles/components/CycleRow.js";
import { CyclesLoadError } from "../../cycles/components/CyclesLoadError.js";
import { CyclesNoMatch } from "../../cycles/components/CyclesNoMatch.js";
import type { CycleFilterDimension } from "../../cycles/lib/cycleFilters.js";
import { filterCyclesByQuery } from "../../cycles/lib/cycleSearch.js";
import { selectCycleMemberDirectory } from "../../cycles/lib/cycleMembers.js";
import { archivedCycleStoreSlot, useArchivedCycleListFilter } from "../api/archivedCycles.js";
import { resolveArchivedCyclesBody } from "../lib/archivedCyclesBody.js";
import { ArchivedCyclePeekPanel } from "./ArchivedCyclePeekPanel.js";
import { ArchivedCyclesEmpty } from "./ArchivedCyclesEmpty.js";
import { ArchivedCyclesSearchBox } from "./ArchivedCyclesSearchBox.js";
import { ArchivedCyclesSkeleton } from "./ArchivedCyclesSkeleton.js";

/** Dimensions the archived tab offers; status stays on the live list. */
const ARCHIVED_CYCLE_DIMENSIONS: readonly CycleFilterDimension[] = ["startDate", "endDate"];

export interface ArchivedCyclesScreenProps {
  workspaceSlug: string;
  projectId: string;
  /** Peeked cycle id from ?peekCycle=; null when no peek is open. */
  peekCycleId: string | null;
  onPeekChange: (cycleId: string | null) => void;
}

export function ArchivedCyclesScreen({
  workspaceSlug,
  projectId,
  peekCycleId,
  onPeekChange,
}: ArchivedCyclesScreenProps): React.ReactElement {
  const slot = archivedCycleStoreSlot(projectId);
  const list = useQuery(archivedCyclesQueryOptions(getAppClient(), workspaceSlug, projectId));
  const project = useProjectRow(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);
  const directoryQuery = usePickerWorkspaceMembers(workspaceSlug, true);
  const query = useCycleSearchStore((state) => state.queryFor(slot));
  const { predicate, clearAll } = useArchivedCycleListFilter(workspaceSlug, projectId);

  const loaded = list.data ?? [];
  const visible = React.useMemo(() => filterCyclesByQuery(loaded, query).filter(predicate), [loaded, query, predicate]);
  const directory = React.useMemo(() => selectCycleMemberDirectory(directoryQuery.data ?? []), [directoryQuery.data]);

  const projectTimeZone = project.data?.timezone ?? "UTC";
  const canMutate = isProjectAuthorized(roles.data?.[projectId] ?? null);
  const peeking = peekCycleId === null ? null : (loaded.find((cycle) => cycle.id === peekCycleId) ?? null);
  const body = resolveArchivedCyclesBody({
    listPending: list.isPending,
    listError: list.isError,
    totalCount: loaded.length,
    visibleCount: visible.length,
    query,
  });

  const onPeek = (cycleId: string): void => {
    onPeekChange(cycleId === peekCycleId ? null : cycleId);
  };

  return (
    <div data-testid="archived-cycles-screen" className="flex min-h-0 flex-1 flex-col gap-(--space-4)">
      <div data-testid="archived-cycles-header" className="flex items-center gap-(--space-2)">
        <ArchivedCyclesSearchBox projectId={projectId} />
        <span data-testid="archived-cycles-header-spacer" aria-hidden="true" className="flex-1" />
        <CycleFilterMenu projectId={slot} offered={ARCHIVED_CYCLE_DIMENSIONS} />
      </div>
      <CycleFilterChips projectId={slot} />
      {body === "skeleton" ? <ArchivedCyclesSkeleton /> : null}
      {body === "error" ? (
        <CyclesLoadError
          onRetry={() => {
            void list.refetch();
          }}
        />
      ) : null}
      {body === "zero" ? <ArchivedCyclesEmpty /> : null}
      {body === "noMatchSearch" || body === "noMatchFilters" ? (
        <CyclesNoMatch
          variant={body === "noMatchSearch" ? "search" : "filters"}
          onClearSearch={() => useCycleSearchStore.getState().clearQuery(slot)}
          onClearFilters={clearAll}
        />
      ) : null}
      {body === "list"
        ? visible.map((cycle) => (
            <CycleRow
              key={cycle.id}
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              cycle={cycle}
              projectTimeZone={projectTimeZone}
              directory={directory}
              showFavoriteMarker={false}
              onPeek={onPeek}
              canMutate={canMutate}
              currentProject={{
                id: projectId,
                name: project.data?.name ?? "",
                identifier: project.data?.identifier ?? "",
              }}
            />
          ))
        : null}
      {peekCycleId === null ? null : (
        <ArchivedCyclePeekPanel
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
