// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail work-item count badge (parity/specs/modules.md §8.1,
// MOD-029). The loaded list total with the pinned explanatory
// tooltip; absent while the module holds no work items.

import { Badge, Tooltip } from "@pidash/kit";
import * as React from "react";

import { moduleBadgeTooltip } from "../lib/moduleDetail.js";

export interface ModuleDetailBadgeProps {
  /** Loaded work-item total; zero renders nothing. */
  total: number;
}

export function ModuleDetailBadge({ total }: ModuleDetailBadgeProps): React.ReactElement | null {
  if (total <= 0) return null;
  return (
    <Tooltip content={moduleBadgeTooltip(total)}>
      <span data-testid="module-detail-badge">
        <Badge>{total}</Badge>
      </span>
    </Tooltip>
  );
}
