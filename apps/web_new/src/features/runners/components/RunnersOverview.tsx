// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners overview (NEWFRONT-540, RUN-002/RUN-010/RUN-011). One table of the
// scope's runners with its pod, status, platform, version and heartbeat, plus
// a Details link and the two destructive row actions behind confirmations,
// plus the pods section above it (NEWFRONT-542) and its three dialogs. The
// add-runner entry arrives with its own slice.

import { Badge, Button } from "@pidash/kit";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import type { Pod, Runner, RunnerPatch } from "@pidash/api-client";
import {
  BUILT_IN_BADGE,
  isEngineRunner,
  runnerDisplayName,
  runnerMachineName,
} from "../../../shared/lib/runnerIdentity.js";
import { notifyError } from "../../../shared/shell/index.js";
import type { PodFilterState } from "../podFilter.js";
import { runnersBase, type RunnersScopeInput } from "../scope.js";
import { runnerDisplayStatus } from "../status.js";
import { CreatePodDialog } from "./CreatePodDialog.js";
import { DeletePodDialog } from "./DeletePodDialog.js";
import { EditPodDialog } from "./EditPodDialog.js";
import { PodMembersPanel } from "./PodMembersPanel.js";
import { REMOVE_ENGINE_LABEL, RunnerConfirmDialog, type RunnerConfirmAction } from "./RunnerConfirmDialog.js";
import { RunnerStatusBadge } from "./RunnerStatusBadge.js";
import { PodFilterBar } from "./RunnersPodFilter.js";
import { RunnersPods } from "./RunnersPods.js";

export const RUNNERS_EMPTY_TEXT = "No runners yet. Add a runner to give this scope its first one.";
export const RUNNER_ACTION_FAILED_TITLE = "Error!";
export const DELETE_RUNNER_FAILED = "Failed to delete runner";
export const REVOKE_RUNNER_FAILED = "Failed to revoke runner";
export const REMOVE_ENGINE_FAILED = "Failed to remove the built-in engine from the project";

/** Revoke only makes sense while credentials exist to invalidate. */
export function canRevokeRunner(runner: Pick<Runner, "status" | "enrolled_at">): boolean {
  return runner.enrolled_at !== null && runner.status !== "revoked";
}

/** Platform cell: "os / arch", or a dash when the runner never reported. */
export function runnerPlatform(runner: Pick<Runner, "os" | "arch">): string {
  if (runner.os === "" && runner.arch === "") return "—";
  return `${runner.os} / ${runner.arch}`;
}

/** Version cell with the dash fallback for pre-enrolment rows. */
export function runnerVersion(runner: Pick<Runner, "runner_version">): string {
  return runner.runner_version === "" ? "—" : runner.runner_version;
}

/**
 * Heartbeat cell: a locale string when the runner has reported, a dash
 * otherwise. The formatter is injected so tests can pin it.
 */
export function runnerHeartbeat(
  runner: Pick<Runner, "last_heartbeat_at">,
  format: (iso: string) => string = (iso) => new Date(iso).toLocaleString()
): string {
  return runner.last_heartbeat_at === null ? "—" : format(runner.last_heartbeat_at);
}

/**
 * Name cell (built-in engine design §11): engines read "Built-in engine ·
 * <machine>" with a Built-in badge; every row names the machine it runs
 * on underneath when one is known.
 */
export function RunnerNameCell({ runner }: { runner: Runner }): React.ReactElement {
  const engine = isEngineRunner(runner);
  const machine = runnerMachineName(runner);
  return (
    <div className="flex flex-col gap-(--space-1)">
      <span className="flex flex-wrap items-center gap-(--space-2)">
        <span className={engine ? "text-(--text)" : "font-mono text-meta text-(--text)"}>
          {runnerDisplayName(runner)}
        </span>
        {engine ? (
          <span data-testid="runner-built-in-badge" className="inline-flex">
            <Badge tone="accent">{BUILT_IN_BADGE}</Badge>
          </span>
        ) : null}
      </span>
      {machine === null || engine ? null : (
        <span data-testid="runner-machine" className="text-meta text-(--text-muted)">
          {machine}
        </span>
      )}
    </div>
  );
}

export interface RunnersOverviewProps {
  scope: RunnersScopeInput;
  /** Rows and the pod selection narrowing them (RUN-012). */
  filter: PodFilterState;
  /** Pods of the scope and the pod read's progress (RUN-012). */
  pods: Pod[] | undefined;
  podsLoading: boolean;
  podsFailed: boolean;
  /** Delete a runner row; built-in engines pass `purgeLocal: false` to keep their folder. */
  onDelete: (runnerId: string, options?: { purgeLocal?: boolean }) => Promise<void>;
  /** Revoke a runner; also backs a built-in engine's "Remove from project". */
  onRevoke: (runnerId: string) => Promise<void>;
  /** Header action slot (the add-runner entry mounts here). */
  action?: React.ReactNode;
  /** Every runner of the scope for the pod members panel; the filtered rows when absent. */
  allRunners?: Runner[] | undefined;
  /** Signed-in user; pod membership actions are owner-only. */
  userId?: string | null | undefined;
  /** Pod membership writer; without it the members panel is read-only. */
  onUpdateRunner?: ((runnerId: string, patch: RunnerPatch) => Promise<unknown>) | undefined;
}

/** The confirmation waiting for an answer; the runner row is its target. */
type PendingAction = { action: RunnerConfirmAction; runner: Runner };

/** Which row actions a runner offers (design §11). */
export interface RunnerRowActions {
  revoke: boolean;
  /** A built-in engine's "Remove from project": revoke, folder kept. */
  remove: boolean;
  delete: boolean;
}

/**
 * Own-agent runners keep Revoke and Delete. A built-in engine is the
 * app's own runner: while live it offers only "Remove from project",
 * which revokes the row and keeps its folder; once removed, Delete
 * clears the row, still without touching the folder.
 */
export function runnerRowActions(
  runner: Pick<Runner, "status" | "enrolled_at" | "revoked_at" | "provisioning">
): RunnerRowActions {
  if (isEngineRunner(runner)) {
    const removed = runner.revoked_at !== null || runner.status === "revoked";
    return { revoke: false, remove: !removed, delete: removed };
  }
  return { revoke: canRevokeRunner(runner), remove: false, delete: true };
}

type PodDialog = { mode: "create" } | { mode: "edit" | "delete"; pod: Pod } | null;

export function RunnersOverview({
  scope,
  filter,
  pods,
  podsLoading,
  podsFailed,
  onDelete,
  onRevoke,
  action,
  allRunners,
  userId = null,
  onUpdateRunner,
}: RunnersOverviewProps): React.ReactElement {
  const navigate = useNavigate();
  const runners = filter.runners;
  const base = runnersBase(scope);
  const [pending, setPending] = React.useState<PendingAction | null>(null);
  const [working, setWorking] = React.useState(false);
  const [podDialog, setPodDialog] = React.useState<PodDialog>(null);

  function closePodDialog(): void {
    setPodDialog(null);
  }

  function afterPodDeleted(podId: string): void {
    // A filter on a pod that no longer exists would hide every row.
    if (filter.selectedPodId === podId) filter.clear();
  }

  async function confirm(action: PendingAction): Promise<void> {
    setWorking(true);
    try {
      if (action.action === "delete") {
        // A built-in engine's folder is kept (design §11): never purge it.
        await (isEngineRunner(action.runner)
          ? onDelete(action.runner.id, { purgeLocal: false })
          : onDelete(action.runner.id));
      } else {
        await onRevoke(action.runner.id);
      }
      setPending(null);
    } catch {
      // The dialog stays up for a retry; the toast carries the reason.
      notifyError(
        RUNNER_ACTION_FAILED_TITLE,
        action.action === "delete"
          ? DELETE_RUNNER_FAILED
          : action.action === "remove"
            ? REMOVE_ENGINE_FAILED
            : REVOKE_RUNNER_FAILED
      );
    } finally {
      setWorking(false);
    }
  }

  return (
    <section
      aria-label="AI Workers"
      data-testid="runners-overview"
      className="flex flex-col gap-(--space-6) p-(--space-6)"
    >
      <div className="flex items-center justify-between gap-(--space-3)">
        <h1 className="text-h2 text-(--text)">AI Workers</h1>
        {action}
      </div>
      <RunnersPods
        pods={pods}
        loading={podsLoading}
        failed={podsFailed}
        selectedPodId={filter.selectedPodId}
        onToggle={filter.toggle}
        onCreate={() => setPodDialog({ mode: "create" })}
        onEdit={(pod) => setPodDialog({ mode: "edit", pod })}
        onDelete={(pod) => setPodDialog({ mode: "delete", pod })}
      />
      {filter.selectedPod === null ? null : <PodFilterBar selectedPod={filter.selectedPod} onClear={filter.clear} />}
      {filter.selectedPod === null ? null : (
        <PodMembersPanel
          pod={filter.selectedPod}
          runners={allRunners ?? runners}
          pods={pods ?? []}
          userId={onUpdateRunner === undefined ? null : userId}
          onUpdate={onUpdateRunner ?? (async () => undefined)}
        />
      )}
      <div className="overflow-x-auto rounded-(--radius-control) border border-(--border)">
        <table className="text-body w-full text-left">
          <thead className="bg-(--subtle) text-(--text-muted)">
            <tr>
              <th scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                Name
              </th>
              <th scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                Pod
              </th>
              <th scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                Status
              </th>
              <th scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                OS / Arch
              </th>
              <th scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                Version
              </th>
              <th scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                Last heartbeat
              </th>
              <th scope="col" className="px-(--space-3) py-(--space-2)">
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {runners.map((runner) => {
              const actions = runnerRowActions(runner);
              return (
                <tr key={runner.id} data-runner={runner.name} className="border-t border-(--border)">
                  <td className="px-(--space-3) py-(--space-2)">
                    <RunnerNameCell runner={runner} />
                  </td>
                  <td className="px-(--space-3) py-(--space-2)">{runner.pod_detail.name}</td>
                  <td className="px-(--space-3) py-(--space-2)">
                    <RunnerStatusBadge status={runnerDisplayStatus(runner)} />
                  </td>
                  <td className="px-(--space-3) py-(--space-2)">{runnerPlatform(runner)}</td>
                  <td className="px-(--space-3) py-(--space-2)">{runnerVersion(runner)}</td>
                  <td className="px-(--space-3) py-(--space-2)">{runnerHeartbeat(runner)}</td>
                  <td className="px-(--space-3) py-(--space-2)">
                    <div className="flex justify-end gap-(--space-2)">
                      <Button size="small" onClick={() => void navigate({ to: `${base}/detail/${runner.id}` })}>
                        Details
                      </Button>
                      {actions.revoke ? (
                        <Button size="small" variant="danger" onClick={() => setPending({ action: "revoke", runner })}>
                          Revoke
                        </Button>
                      ) : null}
                      {actions.remove ? (
                        <Button size="small" variant="danger" onClick={() => setPending({ action: "remove", runner })}>
                          {REMOVE_ENGINE_LABEL}
                        </Button>
                      ) : null}
                      {actions.delete ? (
                        <Button size="small" variant="danger" onClick={() => setPending({ action: "delete", runner })}>
                          Delete
                        </Button>
                      ) : null}
                    </div>
                  </td>
                </tr>
              );
            })}
            {runners.length === 0 ? (
              <tr>
                <td colSpan={7} className="px-(--space-3) py-(--space-8) text-center text-(--text-muted)">
                  {RUNNERS_EMPTY_TEXT}
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>
      <CreatePodDialog
        workspaceSlug={scope.workspaceSlug}
        open={podDialog?.mode === "create"}
        onClose={closePodDialog}
      />
      <EditPodDialog
        workspaceSlug={scope.workspaceSlug}
        pod={podDialog?.mode === "edit" ? podDialog.pod : null}
        onClose={closePodDialog}
      />
      <DeletePodDialog
        workspaceSlug={scope.workspaceSlug}
        pod={podDialog?.mode === "delete" ? podDialog.pod : null}
        onCancel={closePodDialog}
        onDeleted={afterPodDeleted}
      />
      {pending === null ? null : (
        <RunnerConfirmDialog
          open
          action={pending.action}
          runnerName={runnerDisplayName(pending.runner)}
          engine={isEngineRunner(pending.runner)}
          pending={working}
          onCancel={() => setPending(null)}
          onConfirm={() => void confirm(pending)}
        />
      )}
    </section>
  );
}
