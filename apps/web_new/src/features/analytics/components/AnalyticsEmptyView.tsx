// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics no-project empty view (parity/specs/analytics.md §3,
// ANL-003). Replaces tabs, picker, and panels when the caller has no
// selectable project. Creation opens the shared project dialog and
// is disabled for guests; the dialog's own write invalidates the
// project list, which carries the screen out of this view.

import { useQuery } from "@tanstack/react-query";
import { EmptyState } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { CreateProjectDialog, workspaceMembersQueryOptions } from "../../projects/index.js";

export interface AnalyticsEmptyViewProps {
  workspaceSlug: string;
  /** Admin or member standing; guests see the action disabled. */
  canCreate: boolean;
}

export function AnalyticsEmptyView({ workspaceSlug, canCreate }: AnalyticsEmptyViewProps): React.ReactElement {
  const [createOpen, setCreateOpen] = React.useState(false);
  const members = useQuery({ ...workspaceMembersQueryOptions(getAppClient(), workspaceSlug), enabled: createOpen });
  return (
    <div data-testid="analytics-empty">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "empty" }}
        title="No project"
        description="Analytics measures the work inside projects, and this workspace does not have any yet. Create the first project to get started."
        primaryAction={{
          label: "Create a project",
          onSelect: () => setCreateOpen(true),
          disabled: !canCreate,
        }}
      />
      <CreateProjectDialog
        workspaceSlug={workspaceSlug}
        members={members.data}
        open={createOpen}
        onOpenChange={setCreateOpen}
      />
    </div>
  );
}
