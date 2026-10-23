// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One module in the timeline layout (parity/specs/modules.md §3.4 and §6,
// MOD-011 identity). The timeline's date grid, bars, drag and resize belong
// to slice I (MOD-021); this slice renders the sidebar row identity — the
// name anchored to the detail page — in the same layout frame, so the
// skeleton and the loaded state agree on shape.

import { Link } from "@tanstack/react-router";
import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";

import { moduleDetailHref } from "../lib/moduleList.js";

export interface ModuleTimelineRowProps {
  workspaceSlug: string;
  projectId: string;
  module: ModuleRow;
  /** The chart track beside the name. Slot owner: NEWFRONT-801. */
  track?: React.ReactNode;
}

export function ModuleTimelineRow({
  workspaceSlug,
  projectId,
  module,
  track,
}: ModuleTimelineRowProps): React.ReactElement {
  return (
    <li
      data-testid="modules-timeline-row"
      data-module-id={module.id}
      data-module-name={module.name}
      data-module-status={module.status}
      className="flex h-12 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
    >
      <Link
        to={moduleDetailHref(workspaceSlug, projectId, module.id)}
        data-testid="modules-timeline-row-link"
        className="flex w-48 min-w-0 shrink-0 items-center"
      >
        <span data-testid="modules-timeline-row-name" className="text-body min-w-0 truncate font-medium text-(--text)">
          {module.name}
        </span>
      </Link>
      <div className="min-w-0 flex-1">{track}</div>
    </li>
  );
}
