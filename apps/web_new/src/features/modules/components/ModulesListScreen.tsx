// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project modules list (parity/specs/modules.md §2.1–§2.7 and §3.1–§3.3,
// MOD-001–011). Reads the project facts for the name, the feature switch
// and the zone, the modules collection for the body and the role map for
// the gate's shortcut; sets the tab title; and composes exactly one body
// state — gate, skeleton or content. The facts read is the modules
// feature's own tolerant projection: a project record without
// `module_view` parses and reads as off, so it gates instead of leaving
// the skeleton up (MOD-002). No list read runs while the gate shows, and
// the gate never flashes a skeleton first. The layout, search, ordering
// and filters are the viewer's per-project browser record (§13.3.1). This
// slice owns the header controls, the applied-chip strip and the
// no-match container; the list context exposes the chosen sort to later
// slices (the timeline's manual reorder). Slice F adds the two creation
// entry points — the header action and the zero-state shortcut — and the
// dialog provider whose openers every later menu calls. An empty project
// renders the onboarding state instead of an empty list. The create action
// is mounted only for admin and member standing, so guests never see it.
//
// Slice D adds the peek: the screen takes the `peekModule` key the route
// validated, hands every row and card a writer that opens it, and mounts
// the shared peek panel below the header while the key names a module
// (§5, MOD-019). Closing writes the key away; the screen never decides a
// panel's own address.

import type { ModuleRow, WorkspaceMember } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { meQueryOptions } from "../../../core/session/queries.js";
import { useDocumentHead, useNarrowViewport } from "../../../shared/shell/index.js";
import { useProjectRoles } from "../../auth/index.js";
import { isProjectAdmin, isProjectAuthorized, workspaceMembersQueryOptions } from "../../projects/index.js";
import { moduleProjectFactsQueryOptions, moduleProjectIdentifier, moduleViewEnabled } from "../api/projectFacts.js";
import { modulesListQueryOptions } from "../api/queries.js";
import { MODULES_CRUMB, modulesFeatureHref, modulesTitle, resolveModulesListBody } from "../lib/moduleList.js";
import { resolveCompactLayout, type ModulesLayout } from "../lib/moduleLayouts.js";
import { useModuleLayout } from "../lib/listLayoutStore.js";
import { hasModuleFilters, type ModuleListFilters, type ModuleSort } from "../lib/moduleListControls.js";
import { useModuleListDisplay, type ModuleListDisplay } from "../lib/moduleListDisplayStore.js";
import { ModuleCreateAction } from "./dialogs/ModuleCreateAction.js";
import { ModuleDialogProvider, useModuleDialogs } from "./dialogs/ModuleDialogProvider.js";
import { ModuleRowMenu } from "./menus/ModuleRowMenu.js";
import { ModuleListProvider } from "./moduleListContext.js";
import { ModulesFeatureGate } from "./ModulesFeatureGate.js";
import { ModulesFilterChips } from "./ModulesFilterChips.js";
import { ModulesFilterMenu } from "./ModulesFilterMenu.js";
import { ModulesListBody } from "./ModulesListBody.js";
import { ModulesListHeader } from "./ModulesListHeader.js";
import { ModulesListSkeleton } from "./ModulesListSkeleton.js";
import { ModulePeekPanel } from "./ModulePeekPanel.js";
import { ModulesSearch } from "./ModulesSearch.js";
import { ModulesSortMenu } from "./ModulesSortMenu.js";
import { ModulesZeroState } from "./ModulesZeroState.js";

/**
 * The screen's one page slot: header (or nothing) over the body state, with
 * the peek panel mounted beside them once a module is peeked.
 */
function listSection(header: React.ReactNode, body: React.ReactNode, peek: React.ReactNode = null): React.ReactElement {
  return (
    <section
      aria-label={MODULES_CRUMB}
      data-testid="modules-list-screen"
      className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
    >
      {header}
      {body}
      {peek}
    </section>
  );
}

/**
 * The settled list surface: header controls plus the body the screen chose.
 * It is a stable top-level component on purpose — opening the filters and
 * picking a value re-renders the screen, and an inline component type
 * would remount the panel (closing it) on every selection.
 */
interface ModulesListSettledProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  layout: ModulesLayout;
  onLayoutChange: (layout: ModulesLayout) => void;
  narrow: boolean;
  createAllowed: boolean;
  display: ModuleListDisplay;
  onQueryChange: (query: string) => void;
  onSortChange: (sort: ModuleSort) => void;
  onFiltersChange: (filters: ModuleListFilters) => void;
  members: readonly WorkspaceMember[] | undefined;
  meId: string | null;
  /** The viewer's project role number; the row/card accessories read it. */
  memberRole: number | null;
  shownLayout: ModulesLayout;
  loading: boolean;
  modules: readonly ModuleRow[];
  /** The list read settled empty — onboarding replaces the empty body. */
  empty: boolean;
  /** The peeked module id from the address; null when no panel is open. */
  peekModuleId: string | null;
  /** Write the peek key: an id opens, null closes. */
  onPeekChange: (moduleId: string | null) => void;
  /** Archived mode for the rows the body renders (slice G's screen). */
  archived?: boolean | undefined;
  /**
   * Per-row quick menu slot. Absent, the screen mounts the shared row
   * menu in the matching mode; slice G wires its archive opener on the
   * live list's default menu (MOD-026), never a second menu.
   */
  rowMenu?: ((module: ModuleRow) => React.ReactNode) | undefined;
  children?: React.ReactNode;
}

function ModulesListSettled({
  workspaceSlug,
  projectId,
  projectName,
  layout,
  onLayoutChange,
  narrow,
  createAllowed,
  display,
  onQueryChange,
  onSortChange,
  onFiltersChange,
  members,
  meId,
  memberRole,
  shownLayout,
  loading,
  modules,
  empty,
  peekModuleId,
  onPeekChange,
  archived,
  rowMenu,
  children,
}: ModulesListSettledProps): React.ReactElement {
  const dialogs = useModuleDialogs();
  const openCreate = (): void => {
    dialogs?.openCreate();
  };
  // The trigger toggles: pressing the info control of the module already
  // open closes the panel, which keeps the control honest when it stays in
  // view on a narrow screen.
  const openPeek = (moduleId: string): void => {
    onPeekChange(moduleId === peekModuleId ? null : moduleId);
  };
  // The list already knows the peeked module's name, so the panel's title
  // is readable while the detail read lands.
  const peekedName = peekModuleId === null ? undefined : modules.find((module) => module.id === peekModuleId)?.name;
  const peek =
    peekModuleId === null || archived === true ? null : (
      <ModulePeekPanel
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        moduleId={peekModuleId}
        fallbackName={peekedName}
        onClose={() => {
          onPeekChange(null);
        }}
      />
    );
  // The live list mounts the shared quick menu (slice H) on every row
  // and card; editors get the full entries, guests the link actions.
  // Archived hosts pass their own menu to wire the archive opener. On the
  // live list the menu's Archive entry opens this screen's confirmation
  // (MOD-026), so the entry never dead-ends.
  const openArchive = (module: ModuleRow): void => {
    dialogs?.openArchive(module);
  };
  const menuForRow =
    rowMenu ??
    ((module: ModuleRow) => (
      <ModuleRowMenu
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        module={module}
        canEdit={createAllowed}
        archived={archived}
        onArchive={openArchive}
      />
    ));
  const body =
    children !== undefined ? (
      children
    ) : loading ? (
      <ModulesListSkeleton layout={shownLayout} />
    ) : empty ? (
      // Onboarding is for a project that truly has no modules; a failed
      // read is not proof of that, so it keeps the plain list shell.
      <ModulesZeroState canCreate={createAllowed} onCreate={openCreate} />
    ) : (
      <ModulesListBody
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        layout={shownLayout}
        modules={modules}
        peekModuleId={peekModuleId}
        onOpenPeek={openPeek}
        archived={archived}
        members={members}
        memberRole={memberRole}
        rowMenu={menuForRow}
      />
    );
  return listSection(
    <ModulesListHeader
      projectName={projectName}
      layout={layout}
      onLayoutChange={onLayoutChange}
      createControl={createAllowed ? <ModuleCreateAction compact={narrow} onCreate={openCreate} /> : null}
      searchControl={<ModulesSearch query={display.query} onQueryChange={onQueryChange} />}
      orderControl={<ModulesSortMenu sort={display.sort} onSortChange={onSortChange} />}
      filterControl={
        <ModulesFilterMenu filters={display.filters} members={members} meId={meId} onFiltersChange={onFiltersChange} />
      }
      chips={
        <ModulesFilterChips filters={display.filters} members={members} meId={meId} onFiltersChange={onFiltersChange} />
      }
    />,
    body,
    peek
  );
}

export interface ModulesListScreenProps {
  workspaceSlug: string;
  projectId: string;
  /**
   * The module named by the address's peek key, already validated by the
   * route; null when nothing is peeked. The screen never reads the address
   * itself, so an embedding host (slice G's archives screen) can open a
   * peek without a route of its own.
   */
  peekModuleId?: string | null;
  /** Write the peek key: an id opens, null closes. */
  onPeekChange?: (moduleId: string | null) => void;
  /** Render rows in archived mode, where a body click opens the peek. */
  archived?: boolean;
  /**
   * Per-row quick menu slot. Absent, the screen mounts the shared row
   * menu in the matching mode; slice G wires its archive opener on the
   * live list's default menu (MOD-026), never a second menu.
   */
  rowMenu?: (module: ModuleRow) => React.ReactNode;
  /**
   * The list body. Absent, the screen renders its own rows, cards or
   * timeline rows; slices B and F wrap this set with narrowing and the
   * empty containers.
   */
  children?: React.ReactNode;
}

function ignorePeekChange(): void {
  // A host that renders rows without a peek address still gets working
  // info controls; they simply have nowhere to write.
}

export function ModulesListScreen({
  workspaceSlug,
  projectId,
  peekModuleId = null,
  onPeekChange = ignorePeekChange,
  archived,
  rowMenu,
  children,
}: ModulesListScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const narrow = useNarrowViewport();
  const project = useQuery(moduleProjectFactsQueryOptions(getAppClient(), workspaceSlug, projectId));
  // Only a literal `true` opens the list; an absent flag is off (§2.2).
  const gateOff = project.data !== undefined && !moduleViewEnabled(project.data.module_view);
  const modules = useQuery({
    ...modulesListQueryOptions(getAppClient(), workspaceSlug, projectId),
    enabled: project.isSuccess && !gateOff,
  });
  const roles = useProjectRoles(workspaceSlug);
  const { layout, setLayout } = useModuleLayout(projectId);
  const { display, setQuery, setSort, setFilters } = useModuleListDisplay(projectId);
  // The lead/member options are the workspace directory; the current user
  // is offered first as "You".
  const directory = useQuery(workspaceMembersQueryOptions(getAppClient(), workspaceSlug));
  const me = useQuery(meQueryOptions(getAppClient()));
  const filtersActive = hasModuleFilters(display.filters);

  useDocumentHead({
    title: project.data === undefined ? MODULES_CRUMB : modulesTitle(project.data.name),
    robots: "noindex, nofollow",
  });

  const state = resolveModulesListBody({
    projectPending: project.isPending,
    gateOff,
    modulesPending: modules.isPending,
  });
  // Small screens never render the timeline from the list; the stored
  // choice survives for the next wide visit.
  const shownLayout = narrow ? resolveCompactLayout(layout) : layout;

  const context = {
    query: display.query,
    setQuery,
    sort: display.sort,
    setSort,
    filters: display.filters,
    setFilters,
    filtersActive,
  };
  const createAllowed = isProjectAuthorized(roles.data?.[projectId] ?? null);
  const isAdmin = isProjectAdmin(roles.data?.[projectId] ?? null);

  // Before the project resolves there is nothing to gate on: the
  // skeleton stands alone, with no header and no reads behind it.
  if (project.data === undefined) {
    return (
      <ModuleListProvider value={context}>
        {listSection(null, <ModulesListSkeleton layout={shownLayout} />)}
      </ModuleListProvider>
    );
  }
  if (state === "gate") {
    return (
      <ModuleListProvider value={context}>
        {listSection(
          null,
          <ModulesFeatureGate
            isAdmin={isAdmin}
            onManageFeatures={() => {
              void navigate({ to: modulesFeatureHref(workspaceSlug, projectId) });
            }}
          />
        )}
      </ModuleListProvider>
    );
  }

  return (
    <ModuleDialogProvider
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      currentProject={{
        id: projectId,
        name: project.data.name,
        identifier: moduleProjectIdentifier(project.data),
      }}
    >
      <ModuleListProvider value={context}>
        <ModulesListSettled
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          projectName={project.data.name}
          layout={layout}
          onLayoutChange={setLayout}
          narrow={narrow}
          createAllowed={createAllowed}
          display={display}
          onQueryChange={setQuery}
          onSortChange={setSort}
          onFiltersChange={setFilters}
          members={directory.data}
          meId={me.data?.id ?? null}
          memberRole={roles.data?.[projectId] ?? null}
          shownLayout={shownLayout}
          loading={state === "skeleton"}
          modules={modules.data ?? []}
          empty={modules.isSuccess && modules.data.length === 0}
          peekModuleId={peekModuleId}
          onPeekChange={onPeekChange}
          archived={archived}
          {...(rowMenu === undefined ? {} : { rowMenu })}
        >
          {children}
        </ModulesListSettled>
      </ModuleListProvider>
    </ModuleDialogProvider>
  );
}
