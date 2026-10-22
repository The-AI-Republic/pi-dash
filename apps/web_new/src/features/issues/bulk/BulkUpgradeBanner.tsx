// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// OSS bulk-operations banner (parity/specs/issues.md §10, ISS-111). The
// community build offers selection mechanics but no functional toolbar;
// once anything is selected this sticky bar explains the paid capability
// and links to the pricing page. It renders nothing without a selection.

import * as React from "react";

/** Where the banner sends a reader who wants functional bulk editing. */
export const BULK_UPGRADE_HREF = "https://airepublic.com/pricing/pro";

export interface BulkUpgradeBannerProps {
  count: number;
}

export function BulkUpgradeBanner({ count }: BulkUpgradeBannerProps): React.ReactElement | null {
  if (count === 0) return null;
  return (
    <div
      data-testid="bulk-upgrade-banner"
      className="sticky bottom-0 z-20 flex items-center justify-between gap-(--space-4) border-t border-(--accent)/40 bg-(--accent-subtle) px-(--space-4) py-(--space-3)"
    >
      <p className="text-body text-(--text)">
        {count === 1 ? "1 work item selected." : `${count} work items selected.`} Unlock multi-field editing and other
        team controls with the Pro plan.
      </p>
      <a
        href={BULK_UPGRADE_HREF}
        target="_blank"
        rel="noopener noreferrer"
        className="text-body shrink-0 rounded-(--radius-control) bg-(--accent) px-(--space-4) py-(--space-2) font-medium text-(--on-accent) hover:opacity-90"
      >
        Upgrade
      </a>
    </div>
  );
}
