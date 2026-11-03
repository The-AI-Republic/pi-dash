// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-creation restriction toggle (ADM-037, from
// parity/specs/admin.md §5.1): a switch bound to the
// DISABLE_WORKSPACE_CREATION configuration flag. On, only the admin
// may create workspaces; off, creation follows the main app's rules.
// The write persists immediately with a progress notice resolving to
// success or failure. The row is a skeleton while configuration
// loads, and the switch locks while a save is in flight.
import { Button, Skeleton, Switch } from "@pidash/kit";
import * as React from "react";

import { useInstanceConfigurations, useRestrictWorkspaceCreation } from "../hooks.js";
import {
  DISABLE_WORKSPACE_CREATION_KEY,
  isWorkspaceCreationRestricted,
  selectConfigurationValue,
} from "../workspace.js";

export function WorkspaceCreationToggle(): React.ReactElement {
  const configurations = useInstanceConfigurations();
  const save = useRestrictWorkspaceCreation();
  return (
    <section
      aria-labelledby="admin-workspace-toggle-heading"
      data-testid="admin-workspace-toggle"
      className="flex flex-col gap-(--space-2)"
    >
      <div className="flex items-center justify-between gap-(--space-4) rounded-(--radius-card) border border-(--border) bg-(--bg) p-(--space-4)">
        <div className="flex min-w-0 flex-col gap-(--space-1)">
          <h2 id="admin-workspace-toggle-heading" className="text-emphasis font-medium text-(--text)">
            Restrict workspace creation
          </h2>
          <p className="text-body text-(--text-muted)">
            When on, only instance admins can create workspaces. Everyone else must be invited.
          </p>
        </div>
        {configurations.isPending ? (
          <span
            role="status"
            aria-label="Loading workspace creation setting"
            data-testid="admin-workspace-toggle-skeleton"
          >
            <Skeleton width="36px" height="20px" round />
          </span>
        ) : configurations.isError ? (
          <Button
            type="button"
            variant="secondary"
            size="small"
            onClick={() => void configurations.refetch()}
            data-testid="admin-workspace-toggle-retry"
          >
            Try again
          </Button>
        ) : (
          <Switch
            checked={isWorkspaceCreationRestricted(
              selectConfigurationValue(configurations.data, DISABLE_WORKSPACE_CREATION_KEY)
            )}
            disabled={save.isPending}
            onCheckedChange={(next) => save.mutate(next)}
            label="Restrict workspace creation"
          />
        )}
      </div>
      {configurations.isError ? (
        <p role="alert" data-testid="admin-workspace-toggle-error" className="text-body text-(--danger)">
          The workspace creation setting could not be loaded.
        </p>
      ) : null}
    </section>
  );
}
