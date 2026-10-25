// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list content (parity/specs/modules.md §2.1, §2.4–§2.7,
// §3.3–§3.6, MOD-001/011 and slice B's MOD-004/005/006/007/010). One
// module set, narrowed by the header's name search and filters and ordered
// by its sort, rendered three ways: rows for list, cards for board, the
// timeline chart for timeline. When modules exist but the narrowing leaves
// none, the no-match container renders instead of the list — the
// underlying set is untouched, so clearing the header restores every
// module in its order. An empty project is slice F's zero state, never
// this container.
//
// Slice D (MOD-012/019/020) supplies the row's peek trigger and archived
// mode, the card's peek trigger, and the peek itself: a row or card press
// reports the module id to the screen, which owns the `peekModule` key.
// The board grid loses a wide column while the panel is open so a card
// behind it stays readable (§5). Slices E–H fill the accessory slots the
// row and card hand them; the timeline chart belongs to slice I (MOD-021).

import type { ModuleRow, WorkspaceMember } from "@pidash/api-client";
import * as React from "react";

import type { ModulesLayout } from "../lib/moduleLayouts.js";
import { resolveModuleLead } from "../lib/moduleIdentity.js";
import { moduleBoardGridClass } from "../lib/modulePeek.js";
import {
  DEFAULT_MODULE_SORT,
  moduleListContentState,
  orderModules,
  visibleModules,
  type ModuleListFilters,
  type ModuleSort,
} from "../lib/moduleListControls.js";
import { useModuleListContext } from "./moduleListContext.js";
import { ModuleBoardCard } from "./ModuleBoardCard.js";
import { ModuleListRow, moduleRowHoverClass } from "./ModuleListRow.js";
import { ModulesNoMatch } from "./ModulesNoMatch.js";
import { ModulesTimeline } from "./timeline/ModulesTimeline.js";

export interface ModulesListNarrowing {
  /** Case-insensitive name search. */
  query: string;
  sort: ModuleSort;
  filters: ModuleListFilters;
}

export interface ModulesListBodyProps extends Partial<ModulesListNarrowing> {
  workspaceSlug: string;
  projectId: string;
  /** The settled layout: list, board or an effective compact layout. */
  layout: ModulesLayout;
  /** The project's non-archived modules, in server order. */
  modules: readonly ModuleRow[];
  /** Clock anchor for the relative date filters; tests pin it. */
  now?: Date | undefined;
  /** The module the peek panel shows; null when it is closed. */
  peekModuleId?: string | null | undefined;
  /** Open this module's peek (a row's or card's info control, an archived row). */
  onOpenPeek?: ((moduleId: string) => void) | undefined;
  /** Archived mode for the rows the body renders (slice G's screen). */
  archived?: boolean | undefined;
  /** Keep row actions visible at every width (a narrow host, tests). */
  alwaysShowActions?: boolean | undefined;
  /** Per-row quick menu slot (slice H); archived hosts reuse the same slot. */
  rowMenu?: ((module: ModuleRow) => React.ReactNode) | undefined;
  /**
   * The workspace directory the board card resolves a lead against (§4.3);
   * absent, every card shows the no-lead mark.
   */
  members?: readonly WorkspaceMember[] | undefined;
}

export interface ModuleBodyRowOptions {
  archived: boolean | undefined;
  alwaysShowActions: boolean | undefined;
  onOpenPeek: ((moduleId: string) => void) | undefined;
  rowMenu: ((module: ModuleRow) => React.ReactNode) | undefined;
  members: readonly WorkspaceMember[] | undefined;
}

/** One rendered module in the list or board layout; later slices extend both. */
export function renderModule(
  layout: ModulesLayout,
  workspaceSlug: string,
  projectId: string,
  module: ModuleRow,
  options?: ModuleBodyRowOptions
): React.ReactElement {
  const shared = { workspaceSlug, projectId, module };
  // The quick menu (slice H) rides the rowMenu slot on every layout: the
  // card mounts it once, the list row mounts it twice — the narrow-screen
  // trigger in `menu`, the hover-revealed desktop trigger in `trailing` —
  // with exactly one visible per breakpoint.
  const menu = options?.rowMenu?.(module);
  if (layout === "board") {
    const lead = resolveModuleLead(module, options?.members);
    return (
      <ModuleBoardCard
        key={module.id}
        {...shared}
        lead={lead.kind === "avatar" ? lead.person : null}
        onOpenPeek={options?.onOpenPeek}
        {...(menu === undefined ? {} : { menu })}
      />
    );
  }
  const desktopMenu =
    menu === undefined ? undefined : (
      <div
        data-testid="modules-row-menu-desktop"
        className={`hidden shrink-0 items-center md:flex ${moduleRowHoverClass(options?.alwaysShowActions ?? false)}`}
      >
        {menu}
      </div>
    );
  return (
    <ModuleListRow
      key={module.id}
      {...shared}
      archived={options?.archived}
      alwaysShowActions={options?.alwaysShowActions}
      onOpenPeek={options?.onOpenPeek}
      trailing={desktopMenu}
      {...(menu === undefined ? {} : { menu })}
    />
  );
}

export function ModulesListBody({
  workspaceSlug,
  projectId,
  layout,
  modules,
  query: queryProp,
  sort: sortProp,
  filters: filtersProp,
  now,
  peekModuleId = null,
  onOpenPeek,
  archived,
  alwaysShowActions,
  rowMenu,
  members,
}: ModulesListBodyProps): React.ReactElement {
  // The screen provides the list context; props stay available so a host
  // (the archived body, tests) can drive the body on its own.
  const context = useModuleListContext();
  const query = queryProp ?? context?.query ?? "";
  const sort = sortProp ?? context?.sort ?? DEFAULT_MODULE_SORT;
  const filters = filtersProp ?? context?.filters;
  const narrowed = React.useMemo(() => {
    const matching = filters === undefined ? [...modules] : visibleModules(modules, query, filters, now);
    return { matching, ordered: orderModules(matching, sort) };
  }, [modules, query, filters, sort, now]);

  if (filters !== undefined && moduleListContentState(modules.length, narrowed.ordered.length) === "no-match") {
    return <ModulesNoMatch />;
  }

  if (layout === "timeline") {
    return (
      <div data-testid="modules-list" data-layout="timeline" className="flex min-h-0 flex-1 flex-col">
        <ModulesTimeline workspaceSlug={workspaceSlug} projectId={projectId} modules={narrowed.ordered} sort={sort} />
      </div>
    );
  }

  const rowOptions: ModuleBodyRowOptions = { archived, alwaysShowActions, onOpenPeek, rowMenu, members };
  const listClass = layout === "board" ? moduleBoardGridClass({ peekOpen: peekModuleId !== null }) : "flex flex-col";
  return (
    <div
      data-testid="modules-list"
      data-layout={layout}
      data-peek-open={peekModuleId === null ? "false" : "true"}
      className={listClass}
    >
      <ul aria-label="Modules" className={layout === "board" ? "contents" : "flex flex-col"}>
        {narrowed.ordered.map((module) => renderModule(layout, workspaceSlug, projectId, module, rowOptions))}
      </ul>
    </div>
  );
}
