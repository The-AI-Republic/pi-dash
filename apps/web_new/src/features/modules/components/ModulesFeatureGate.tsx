// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules feature gate (parity/specs/modules.md §2.2, MOD-002). Replaces
// the whole list — header, rows, chips and switcher — while the project's
// modules switch is off. Every role reads the art and the explanation; the
// shortcut stays rendered but disabled without project admin standing,
// because only an admin can flip the switch.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface ModulesFeatureGateProps {
  /** Project admin standing: the shortcut stays disabled without it. */
  isAdmin: boolean;
  onManageFeatures: () => void;
}

export function ModulesFeatureGate({ isAdmin, onManageFeatures }: ModulesFeatureGateProps): React.ReactElement {
  return (
    <div data-testid="modules-feature-gate">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "disabled" }}
        title="Modules are not enabled for this project."
        description="Modules group work items into named containers. A project admin can switch them on from the project's feature settings, and this tab then lists every module."
        primaryAction={{ label: "Manage features", onSelect: () => onManageFeatures(), disabled: !isAdmin }}
      />
    </div>
  );
}
