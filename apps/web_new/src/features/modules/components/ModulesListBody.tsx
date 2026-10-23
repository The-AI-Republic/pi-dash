// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list content (parity/specs/modules.md §2.1, §3.4–§3.6,
// MOD-001/011). One module set, rendered three ways: rows for list, cards
// for board, name rows for timeline. This slice renders identity only and
// its default order (name, ascending, natural); the search, ordering and
// filter narrowing of slice B wraps the same set, and slices D–I extend
// the row, card and timeline row in place.

import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";

import { sortModulesByName } from "../lib/moduleList.js";
import type { ModulesLayout } from "../lib/moduleLayouts.js";
import { ModuleBoardCard } from "./ModuleBoardCard.js";
import { ModuleListRow } from "./ModuleListRow.js";
import { ModuleTimelineRow } from "./ModuleTimelineRow.js";

export interface ModulesListBodyProps {
  workspaceSlug: string;
  projectId: string;
  /** The settled layout: list, board or an effective compact layout. */
  layout: ModulesLayout;
  /** The project's non-archived modules, in server order. */
  modules: readonly ModuleRow[];
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
}: ModulesListBodyProps): React.ReactElement {
  const ordered = sortModulesByName(modules);
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
        {ordered.map((module) => renderModule(layout, workspaceSlug, projectId, module))}
      </ul>
    </div>
  );
}
