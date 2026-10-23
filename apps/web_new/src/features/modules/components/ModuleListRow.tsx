// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One module row in the list layout (parity/specs/modules.md §3.5,
// MOD-001 identity). This slice renders the row's navigation identity: the
// name is a real anchor to the module detail page, so copy, middle-click
// and keyboard activation all work natively. The progress marker, inline
// status, dates, lead, favorite and quick menu belong to slices D, E and H;
// they mount through the slots below so nobody forks a second row.

import { Link } from "@tanstack/react-router";
import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";

import { moduleDetailHref } from "../lib/moduleList.js";

export interface ModuleListRowProps {
  workspaceSlug: string;
  projectId: string;
  module: ModuleRow;
  /** Reading-order extras ahead of the name. Slot owner: NEWFRONT-796. */
  leading?: React.ReactNode;
  /** Inline controls after the name. Slot owners: NEWFRONT-796/797/800. */
  trailing?: React.ReactNode;
}

export function ModuleListRow({
  workspaceSlug,
  projectId,
  module,
  leading,
  trailing,
}: ModuleListRowProps): React.ReactElement {
  return (
    <li
      data-testid="modules-row"
      data-module-id={module.id}
      data-module-name={module.name}
      data-module-status={module.status}
      className="flex items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-2)"
    >
      {leading}
      <Link
        to={moduleDetailHref(workspaceSlug, projectId, module.id)}
        data-testid="modules-row-link"
        className="flex h-10 min-w-0 flex-1 items-center gap-(--space-3)"
      >
        <span data-testid="modules-row-name" className="text-body min-w-0 flex-1 truncate font-medium text-(--text)">
          {module.name}
        </span>
      </Link>
      {trailing}
    </li>
  );
}
