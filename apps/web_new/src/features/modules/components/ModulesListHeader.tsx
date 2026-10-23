// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules list header (parity/specs/modules.md §2.1, MOD-001/011). The
// trail reads the project name and a trailing Modules crumb; the control
// cluster carries whatever the other slices mount — create, search,
// ordering and filters — plus this slice's layout switcher. The crumb is
// never deeper than the list itself: Modules is the current location.

import { Breadcrumbs } from "@pidash/kit";
import * as React from "react";

import { MODULES_CRUMB } from "../lib/moduleList.js";
import type { ModulesLayout } from "../lib/moduleLayouts.js";
import { ModulesLayoutSwitcher } from "./ModulesLayoutSwitcher.js";

export interface ModulesListHeaderProps {
  /** Project name, the leading crumb. */
  projectName: string;
  /** Active layout choice. */
  layout: ModulesLayout;
  onLayoutChange: (layout: ModulesLayout) => void;
  /** Create action. Slot owner: NEWFRONT-798. */
  createControl?: React.ReactNode;
  /** Expanding name search. Slot owner: NEWFRONT-794. */
  searchControl?: React.ReactNode;
  /** Ordering menu. Slot owner: NEWFRONT-794. */
  orderControl?: React.ReactNode;
  /** Filters menu. Slot owner: NEWFRONT-794. */
  filterControl?: React.ReactNode;
}

export function ModulesListHeader({
  projectName,
  layout,
  onLayoutChange,
  createControl,
  searchControl,
  orderControl,
  filterControl,
}: ModulesListHeaderProps): React.ReactElement {
  return (
    <div data-testid="modules-list-header" className="flex flex-col gap-(--space-3)">
      <div className="flex items-center justify-between gap-(--space-3)">
        <div className="min-w-0 flex-1">
          <Breadcrumbs entries={[{ label: projectName }, { label: MODULES_CRUMB }]} />
        </div>
        {createControl}
      </div>
      <div data-testid="modules-list-controls" className="flex flex-wrap items-center gap-(--space-2)">
        {searchControl}
        {orderControl}
        {filterControl}
        <div className="ml-auto flex items-center gap-(--space-2)">
          <ModulesLayoutSwitcher layout={layout} onLayoutChange={onLayoutChange} />
        </div>
      </div>
    </div>
  );
}
