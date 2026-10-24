// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list content (parity/specs/modules.md §2.1, §2.4–§2.7,
// §3.3–§3.6, MOD-001/011 and slice B's MOD-004/005/006/007/010). One
// module set, narrowed by the header's name search and filters and ordered
// by its sort, rendered three ways: rows for list, cards for board, name
// rows for timeline. When modules exist but the narrowing leaves none, the
// no-match container renders instead of the list — the underlying set is
// untouched, so clearing the header restores every module in its order.
// An empty project is slice F's zero state, never this container. Slices
// D–I extend the row, card and timeline row in place.

import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";

import type { ModulesLayout } from "../lib/moduleLayouts.js";
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
import { ModuleListRow } from "./ModuleListRow.js";
import { ModuleTimelineRow } from "./ModuleTimelineRow.js";
import { ModulesNoMatch } from "./ModulesNoMatch.js";

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
}

/** One rendered module in the active layout; later slices extend the three. */
function renderModule(
  layout: ModulesLayout,
  workspaceSlug: string,
  projectId: string,
  module: ModuleRow
): React.ReactElement {
  const shared = { workspaceSlug, projectId, module };
  if (layout === "board") return <ModuleBoardCard key={module.id} {...shared} />;
  if (layout === "timeline") return <ModuleTimelineRow key={module.id} {...shared} />;
  return <ModuleListRow key={module.id} {...shared} />;
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

  const listClass =
    layout === "board" ? "grid grid-cols-1 gap-(--space-4) md:grid-cols-2 xl:grid-cols-3" : "flex flex-col";
  return (
    <div data-testid="modules-list" data-layout={layout} className={listClass}>
      <ul
        aria-label="Modules"
        className={
          layout === "board" ? "contents" : layout === "timeline" ? "flex flex-col overflow-x-auto" : "flex flex-col"
        }
      >
        {narrowed.ordered.map((module) => renderModule(layout, workspaceSlug, projectId, module))}
      </ul>
    </div>
  );
}
