// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machines route container (NEWFRONT-555, RUN-037; built-in engine
// design §11). Resolves the workspace row, loads the table's data (the
// machines and, for the expanded rows, every runner including built-in
// engines) and hands the screen its writers. Kept out of the route so a
// unit test can mount it with stubs.

import * as React from "react";

import { selectWorkspace, useWorkspaces } from "../../../core/session/index.js";
import { AddRunnerEntry } from "../addRunner/AddRunnerEntry.js";
import { buildRunnersAreaValue, RunnersAreaProvider } from "../area.js";
import {
  useDeleteDevMachine,
  useDevMachineRunners,
  useDevMachines,
  useMergeDevMachine,
  useRevokeDevMachine,
  useRotateDevMachine,
  useSignOutDevMachineEngine,
  useUpdateDevMachine,
} from "../api/devMachines.js";
import { DevMachinesScreen } from "./DevMachinesScreen.js";

export function DevMachinesContainer({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement | null {
  const workspace = selectWorkspace(useWorkspaces().data, workspaceSlug);
  const workspaceId = workspace?.id ?? "";
  const machines = useDevMachines(workspaceSlug, workspace?.id);
  const runners = useDevMachineRunners(workspaceSlug, workspace?.id);
  const rotate = useRotateDevMachine(workspaceSlug, workspaceId);
  const revoke = useRevokeDevMachine(workspaceSlug, workspaceId);
  const remove = useDeleteDevMachine(workspaceSlug, workspaceId);
  const update = useUpdateDevMachine(workspaceSlug, workspaceId);
  const signOutEngine = useSignOutDevMachineEngine(workspaceSlug, workspaceId);
  const merge = useMergeDevMachine(workspaceSlug);
  if (workspace === undefined) return null;
  const area = buildRunnersAreaValue({ workspaceSlug }, workspace.id, "allowed");
  return (
    <DevMachinesScreen
      workspaceSlug={workspaceSlug}
      workspaceName={workspace.name}
      workspaceId={workspace.id}
      actions={
        <RunnersAreaProvider value={area}>
          <AddRunnerEntry />
        </RunnersAreaProvider>
      }
      machines={machines.data}
      loading={machines.isPending}
      failed={machines.isError}
      runners={runners.data}
      onRotate={(id) => rotate.mutateAsync(id)}
      onRevoke={(id) => revoke.mutateAsync(id)}
      onDelete={(id) => remove.mutateAsync(id)}
      onUpdate={(machineId, patch) => update.mutateAsync({ machineId, patch })}
      onSignOutEngine={(id) => signOutEngine.mutateAsync(id)}
      onMerge={(sourceId, targetId) => merge.mutateAsync({ sourceId, targetId })}
    />
  );
}
