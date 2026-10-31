// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work-item count badge (parity/specs/archives.md §1.3,
// ARCH-002). Renders on the work-items tab only, beside the breadcrumb
// trail, and hides entirely at zero. Hovering or focusing it shows how
// many archived work items the project holds.

import { Badge, Tooltip } from "@pidash/kit";
import * as React from "react";

export interface ArchivesCountBadgeProps {
  count: number | null | undefined;
}

export function ArchivesCountBadge({ count }: ArchivesCountBadgeProps): React.ReactElement | null {
  if (count === null || count === undefined || count <= 0) return null;
  const tip = count === 1 ? "1 archived work item" : `${count} archived work items`;
  return (
    <Tooltip content={tip}>
      <span tabIndex={0} data-testid="archives-count-badge">
        <Badge>{String(count)}</Badge>
      </span>
    </Tooltip>
  );
}
