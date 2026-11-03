// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Zero-views onboarding state (parity/specs/views.md §3, VIEW-009). The
// creation shortcut renders enabled for every role that reached the
// list, guests included; the dialog it opens lands with NEWFRONT-721.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface ProjectViewsZeroStateProps {
  /** Opens the create dialog. Slot owner: NEWFRONT-721. */
  onCreateView?: (() => void) | undefined;
}

export function ProjectViewsZeroState({ onCreateView }: ProjectViewsZeroStateProps): React.ReactElement {
  return (
    <div data-testid="project-views-zero">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "empty" }}
        title="Save custom views for your project"
        description="Views are saved filter sets for quick access. Capture a filtered list once, then share it with your teammates."
        primaryAction={{ label: "Create view", onSelect: () => onCreateView?.() }}
      />
    </div>
  );
}
