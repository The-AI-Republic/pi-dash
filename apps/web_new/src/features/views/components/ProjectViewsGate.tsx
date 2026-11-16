// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views feature gate (parity/specs/views.md §3, VIEW-002). Replaces the
// whole list — header, rows and skeleton — while the project's views
// switch is off. The shortcut targets the rendered views feature leaf,
// never the bare features index, which has no page (bug NEWFRONT-247).

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface ProjectViewsGateProps {
  /** Project admin standing: the shortcut stays disabled without it. */
  isAdmin: boolean;
  onManageFeatures: () => void;
}

export function ProjectViewsGate({ isAdmin, onManageFeatures }: ProjectViewsGateProps): React.ReactElement {
  return (
    <div data-testid="project-views-gate">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "disabled" }}
        title="Views are not enabled for the project."
        description="Saved views can be turned on from project settings. Once enabled, this tab lists every view of the project."
        primaryAction={{ label: "Manage features", onSelect: () => onManageFeatures(), disabled: !isAdmin }}
      />
    </div>
  );
}
