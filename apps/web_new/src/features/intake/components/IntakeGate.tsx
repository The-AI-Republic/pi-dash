// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake feature-off gate (parity/specs/intake.md INT-002). Replaces the
// whole body while the project's intake switch is off: an explanation
// and one shortcut into the project's intake feature leaf. The shortcut
// renders for everyone and stays disabled for anyone who is not a
// project admin. No list, detail or reference read exists on this path.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

import { INTAKE_AREA_LABEL } from "../lib/title.js";

export interface IntakeGateProps {
  /** Project admin standing: the shortcut is enabled only with it. */
  canManageFeatures: boolean;
  onManageFeatures: () => void;
}

export function IntakeGate({ canManageFeatures, onManageFeatures }: IntakeGateProps): React.ReactElement {
  return (
    <div data-testid="intake-gate">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "disabled" }}
        title={`${INTAKE_AREA_LABEL} is not enabled for the project.`}
        description="Requests sent to this project are not collected while intake is off. A project admin can turn it on from the project's feature settings."
        primaryAction={{
          label: "Manage features",
          disabled: !canManageFeatures,
          onSelect: () => {
            onManageFeatures();
          },
        }}
      />
    </div>
  );
}
