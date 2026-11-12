// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages feature gate (parity/specs/pages.md §3.1, PAGE-001). Every viewer
// sees the guidance; only a project admin gets an enabled management
// shortcut. The control stays rendered but disabled for everyone else.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface PagesFeatureGateProps {
  isAdmin: boolean;
  onManageFeatures: () => void;
}

export function PagesFeatureGate({ isAdmin, onManageFeatures }: PagesFeatureGateProps): React.ReactElement {
  return (
    <div data-testid="pages-feature-gate">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "disabled" }}
        title="Pages are turned off for this project."
        description="Pages are collaborative documents that live beside your project work. A project admin can switch them on from the project's feature settings."
        primaryAction={{ label: "Manage features", onSelect: () => onManageFeatures(), disabled: !isAdmin }}
      />
    </div>
  );
}
