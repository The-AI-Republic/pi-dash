// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-modules tab content (parity/specs/modules.md §7.3, MOD-027).
// The modules area's view of the project archive: the modules header
// controls in their archived variant — name search, ordering, and filters
// limited to lead, members and the two dates, because an archived module is
// finished — over the shared module list row in archived mode. Rows open
// the archived peek through the address rather than a detail page, and
// their quick menu is slice H's shared menu in archived mode, so Restore
// runs from the one menu. Narrowing lives in the archived screen's own
// browser record, so opening this tab never changes the live list.
//
// The screen reads the archived list on mount and settles from it: a direct
// load or a reload paints the skeleton and then rows or a settled empty,
// never a loader that waits on a visit from somewhere else. The three
// empties stay distinct (no archive, the search hid everything, the filters
// hid everything), and a failed read offers a retry instead of pretending
// the archive is empty.

import type { ModuleRow } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { meQueryOptions } from "../../../core/session/queries.js";
import { useProjectRoles } from "../../auth/index.js";
import { isProjectAuthorized, workspaceMembersQueryOptions } from "../../projects/index.js";
// The modules area is reached through its barrel (Import rules): this
// screen composes its header controls, its row, its menu and its peek
// without reaching into the feature's own files.
import {
  archivedModulesQueryOptions,
  archivedModulesSearched,
  archivedModulesVisible,
  clearModuleFilters,
  ModulePeekPanel,
  ModuleRowMenu,
  ModulesFilterChips,
  ModulesFilterMenu,
  ModulesListBody,
  ModulesSearch,
  ModulesSortMenu,
  resolveArchivedModulesBody,
  toArchivedModuleRow,
  useArchivedModuleDisplay,
} from "../../modules/index.js";
import { ArchivedModulesEmpty } from "./ArchivedModulesEmpty.js";
import { ArchivedModulesSkeleton } from "./ArchivedModulesSkeleton.js";

export interface ArchivedModulesScreenProps {
  workspaceSlug: string;
  projectId: string;
  /** The module named by ?peekModule=; null when no peek is open. */
  peekModuleId: string | null;
  /** Write the peek key: an id opens the panel, null closes it. */
  onPeekChange: (moduleId: string | null) => void;
}

/** Settled read failure: say so and offer a retry, never a false empty. */
function ArchivedModulesLoadError({ onRetry }: { onRetry: () => void }): React.ReactElement {
  return (
    <div
      data-testid="archived-modules-load-error"
      role="alert"
      className="flex flex-col items-center gap-(--space-3) px-(--space-6) py-(--space-12) text-center"
    >
      <h3 className="text-h3 text-(--text)">The archive didn't load</h3>
      <p className="text-body text-(--text-muted)">Check your connection and try again.</p>
      <button
        type="button"
        onClick={onRetry}
        className="text-body cursor-pointer rounded-(--radius-control) font-medium text-(--accent) hover:underline"
      >
        Retry
      </button>
    </div>
  );
}

export function ArchivedModulesScreen({
  workspaceSlug,
  projectId,
  peekModuleId,
  onPeekChange,
}: ArchivedModulesScreenProps): React.ReactElement {
  const list = useQuery(archivedModulesQueryOptions(getAppClient(), workspaceSlug, projectId));
  const roles = useProjectRoles(workspaceSlug);
  const directory = useQuery(workspaceMembersQueryOptions(getAppClient(), workspaceSlug));
  const me = useQuery(meQueryOptions(getAppClient()));
  const { display, setQuery, setSort, setFilters } = useArchivedModuleDisplay(projectId);

  const rows = React.useMemo(() => (list.data ?? []).map(toArchivedModuleRow), [list.data]);
  // Search first, then the archived narrowing, so an empty result can say
  // which of the two hid the rows.
  const searched = React.useMemo(() => archivedModulesSearched(rows, display.query), [rows, display.query]);
  const visible = React.useMemo(
    () => archivedModulesVisible(rows, display.query, display.filters),
    [rows, display.query, display.filters]
  );
  const body = resolveArchivedModulesBody({
    listPending: list.isPending,
    listError: list.isError,
    totalCount: rows.length,
    searchedCount: searched.length,
    visibleCount: visible.length,
  });

  const meId = me.data?.id ?? null;
  const canEdit = isProjectAuthorized(roles.data?.[projectId] ?? null);
  const peekedName = peekModuleId === null ? undefined : rows.find((row) => row.id === peekModuleId)?.name;

  const openPeek = (moduleId: string): void => {
    onPeekChange(moduleId === peekModuleId ? null : moduleId);
  };

  const rowMenu = (module: ModuleRow): React.ReactNode => (
    <ModuleRowMenu workspaceSlug={workspaceSlug} projectId={projectId} module={module} canEdit={canEdit} archived />
  );

  return (
    <div data-testid="archived-modules-screen" className="flex min-h-0 flex-1 flex-col gap-(--space-4)">
      <div data-testid="archived-modules-header" className="flex flex-wrap items-center gap-(--space-2)">
        <ModulesSearch query={display.query} onQueryChange={setQuery} />
        <ModulesSortMenu sort={display.sort} onSortChange={setSort} />
        <ModulesFilterMenu
          filters={display.filters}
          members={directory.data}
          meId={meId}
          onFiltersChange={setFilters}
          showFavorites={false}
          showStatus={false}
        />
        <span data-testid="archived-modules-header-spacer" aria-hidden="true" className="flex-1" />
      </div>
      <ModulesFilterChips
        filters={display.filters}
        members={directory.data}
        meId={meId}
        onFiltersChange={setFilters}
        showFavorites={false}
        showStatus={false}
      />
      {body === "skeleton" ? <ArchivedModulesSkeleton /> : null}
      {body === "error" ? (
        <ArchivedModulesLoadError
          onRetry={() => {
            void list.refetch();
          }}
        />
      ) : null}
      {body === "zero" ? <ArchivedModulesEmpty kind="zero" /> : null}
      {body === "no-match-search" ? <ArchivedModulesEmpty kind="search" onClearSearch={() => setQuery("")} /> : null}
      {body === "no-match-filters" ? (
        <ArchivedModulesEmpty kind="filters" onClearFilters={() => setFilters(clearModuleFilters())} />
      ) : null}
      {body === "rows" ? (
        <ModulesListBody
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          layout="list"
          modules={visible}
          query=""
          sort={display.sort}
          peekModuleId={peekModuleId}
          onOpenPeek={openPeek}
          archived
          rowMenu={rowMenu}
          members={directory.data}
          memberRole={roles.data?.[projectId] ?? null}
        />
      ) : null}
      {peekModuleId === null ? null : (
        <ModulePeekPanel
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          moduleId={peekModuleId}
          fallbackName={peekedName}
          archived
          onClose={() => {
            onPeekChange(null);
          }}
        />
      )}
    </div>
  );
}
