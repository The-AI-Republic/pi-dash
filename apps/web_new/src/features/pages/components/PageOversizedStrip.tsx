// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oversized-content strip (parity/specs/pages.md §8.6, PAGE-063). It sits
// under the toolbar once the backend refuses an update as too large and
// advises the author to split the content across new or nested pages,
// because live sync is paused. Dismissing is optional and lasts until the
// next too-large report.

import { TriangleAlert } from "lucide-react";
import * as React from "react";

export const PAGE_OVERSIZED_TITLE = "Page too large to sync";
export const PAGE_OVERSIZED_BODY = "Live sync is paused. Split this content across new or nested pages to keep saving.";
export const PAGE_OVERSIZED_DISMISS = "Dismiss";

export interface PageOversizedStripProps {
  onDismiss: () => void;
}

export function PageOversizedStrip({ onDismiss }: PageOversizedStripProps): React.ReactElement {
  return (
    <div
      data-testid="page-oversized-strip"
      role="alert"
      className="flex items-center gap-(--space-2) border-b border-(--border) px-(--space-4) py-(--space-2)"
    >
      <TriangleAlert
        size={14}
        strokeWidth={1.5}
        aria-hidden="true"
        className="text-(--warning, var(--text-muted)) shrink-0"
      />
      <p className="text-caption min-w-0 flex-1 text-(--text-muted)">
        <strong className="font-medium text-(--text)">{PAGE_OVERSIZED_TITLE}</strong> {PAGE_OVERSIZED_BODY}
      </p>
      <button
        type="button"
        onClick={onDismiss}
        className="text-caption shrink-0 cursor-pointer rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:text-(--text)"
      >
        {PAGE_OVERSIZED_DISMISS}
      </button>
    </div>
  );
}
