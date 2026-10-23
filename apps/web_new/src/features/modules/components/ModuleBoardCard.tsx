// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One module card in the board layout (parity/specs/modules.md §3.6,
// MOD-001/020 identity). The name is an anchor to the detail page and the
// whole card carries the module's id so the peek, status, dates, lead and
// menus can mount around it. Those controls belong to slices D, E and H and
// arrive through the slots; this slice renders only the identity.

import { Link } from "@tanstack/react-router";
import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";

import { moduleDetailHref } from "../lib/moduleList.js";

export interface ModuleBoardCardProps {
  workspaceSlug: string;
  projectId: string;
  module: ModuleRow;
  /** Status pill and peek trigger row. Slot owners: NEWFRONT-796/797. */
  header?: React.ReactNode;
  /** Counts, lead, progress bar and date range. Slot owners: NEWFRONT-796/797. */
  footer?: React.ReactNode;
  /** Star and quick menu. Slot owners: NEWFRONT-797/800. */
  actions?: React.ReactNode;
}

export function ModuleBoardCard({
  workspaceSlug,
  projectId,
  module,
  header,
  footer,
  actions,
}: ModuleBoardCardProps): React.ReactElement {
  return (
    <li
      data-testid="modules-board-card"
      data-module-id={module.id}
      data-module-name={module.name}
      data-module-status={module.status}
      className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-4)"
    >
      <div className="flex items-start justify-between gap-(--space-3)">
        <Link
          to={moduleDetailHref(workspaceSlug, projectId, module.id)}
          data-testid="modules-board-card-link"
          className="min-w-0 flex-1"
        >
          <span
            data-testid="modules-board-card-name"
            title={module.name}
            className="text-body block truncate font-medium text-(--text)"
          >
            {module.name}
          </span>
        </Link>
        {actions}
      </div>
      {header}
      {footer}
    </li>
  );
}
