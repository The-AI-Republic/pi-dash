// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machines route container (NEWFRONT-555, RUN-037). Resolves the
// workspace row, loads the table's data and hands the screen the three
// writers. Kept out of the route so a unit test can mount it with stubs.

import * as React from "react";

import { selectWorkspace, useWorkspaces } from "../../../core/session/index.js";
import { useDeleteDevMachine, useDevMachines, useRevokeDevMachine, useRotateDevMachine } from "../api/devMachines.js";
import { DevMachinesScreen } from "./DevMachinesScreen.js";

export function DevMachinesContainer({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement | null {
  const workspace = selectWorkspace(useWorkspaces().data, workspaceSlug);
  const machines = useDevMachines(workspaceSlug, workspace?.id);
  const rotate = useRotateDevMachine(workspaceSlug, workspace?.id ?? "");
  const revoke = useRevokeDevMachine(workspaceSlug, workspace?.id ?? "");
  const remove = useDeleteDevMachine(workspaceSlug, workspace?.id ?? "");
  if (workspace === undefined) return null;
  return (
    <DevMachinesScreen
      workspaceSlug={workspaceSlug}
      workspaceName={workspace.name}
      workspaceId={workspace.id}
      machines={machines.data}
      loading={machines.isPending}
      failed={machines.isError}
      onRotate={(id) => rotate.mutateAsync(id)}
      onRevoke={(id) => revoke.mutateAsync(id)}
      onDelete={(id) => remove.mutateAsync(id)}
    />
  );
}
