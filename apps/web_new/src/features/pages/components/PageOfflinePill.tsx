// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Offline pill (parity/specs/pages.md §8.3, PAGE-062). The header shows it
// in place of the sync badge while the browser reports no connectivity
// on an editable page; typing continues locally underneath it.

import { WifiOff } from "lucide-react";
import * as React from "react";

import { PAGE_OFFLINE_LABEL } from "../lib/pageOffline.js";

export function PageOfflinePill(): React.ReactElement {
  return (
    <span
      data-testid="page-offline-pill"
      className="text-caption inline-flex items-center gap-(--space-1) text-(--text-muted)"
    >
      <WifiOff size={12} strokeWidth={1.5} aria-hidden="true" />
      {PAGE_OFFLINE_LABEL}
    </span>
  );
}
