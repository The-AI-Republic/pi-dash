/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { HelpCircle, Plus } from "lucide-react";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { PodService, RunnerService } from "@pi-dash/services";
import type { IPod, IRunner, TRunnerStatus } from "@pi-dash/types";
import type { TBadgeVariant } from "@pi-dash/ui";
import { AlertModalCore, Badge, Button, Tooltip } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { AddRunnerModal } from "@/components/runners/add-runner-modal";
import { CreatePodModal } from "@/components/runners/create-pod-modal";
import { RunnersTabs } from "@/components/runners/runners-tabs";
import { useSelectedPodFilter } from "@/hooks/use-selected-pod-filter";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();
const podService = new PodService();

const STATUS_BADGE_VARIANT: Record<TRunnerStatus, TBadgeVariant> = {
  online: "accent-success",
  busy: "accent-primary",
  offline: "accent-neutral",
  revoked: "accent-warning",
};

const RUNNER_STATUS_I18N_LABELS: Record<TRunnerStatus, string> = {
  online: "online",
  busy: "busy",
  offline: "offline",
  revoked: "revoked",
};

function isRevocable(r: IRunner): boolean {
  return r.status !== "revoked" && r.enrolled_at !== null;
}

const RunnersListPage = observer(function RunnersListPage() {
  const { currentWorkspace } = useWorkspace();
  const { t } = useTranslation();
  const workspaceId = currentWorkspace?.id;
  const workspaceSlug = currentWorkspace?.slug;
  const pageTitle = currentWorkspace?.name
    ? t("{workspace} - AI Agents", { workspace: currentWorkspace.name })
    : t("AI Agents");

  const { data: runners, mutate: mutateRunners } = useSWR<IRunner[]>(
    workspaceId ? ["runners", workspaceId] : null,
    () => service.list(workspaceId),
    { refreshInterval: 5_000 }
  );

  const {
    data: pods,
    error: podsError,
    mutate: mutatePods,
  } = useSWR<IPod[]>(workspaceId ? ["pods", workspaceId] : null, () => podService.list(workspaceId!), {
    refreshInterval: 30_000,
  });

  const [addOpen, setAddOpen] = useState(false);
  const [createPodOpen, setCreatePodOpen] = useState(false);
  const { selectedPodId, setSelectedPodId, filteredRunners, selectedPod } = useSelectedPodFilter(runners, pods);
  const [deleteRunner, setDeleteRunner] = useState<IRunner | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [revokeRunner, setRevokeRunner] = useState<IRunner | null>(null);
  const [revoking, setRevoking] = useState(false);

  async function confirmDeleteRunner() {
    if (!deleteRunner) return;
    setDeleting(true);
    try {
      await service.deleteRunner(deleteRunner.id);
      setDeleteRunner(null);
      mutateRunners();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message: err?.error ?? t("Failed to delete runner"),
      });
    } finally {
      setDeleting(false);
    }
  }

  async function confirmRevokeRunner() {
    if (!revokeRunner) return;
    setRevoking(true);
    try {
      await service.revokeRunner(revokeRunner.id);
      setRevokeRunner(null);
      mutateRunners();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message: err?.error ?? t("Failed to revoke runner"),
      });
    } finally {
      setRevoking(false);
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <PageHead title={pageTitle} />

      <RunnersTabs />

      {/* Header — primary "Add runner" CTA + how-it-works tooltip */}
      <section className="rounded-md border border-subtle p-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-1.5">
              <div className="text-13 font-medium text-primary">{t("Add runner")}</div>
              <Tooltip
                position="bottom"
                tooltipContent={
                  <div className="flex max-w-xs flex-col gap-1 p-1 text-12 whitespace-normal">
                    <div className="font-medium">{t("How to add a runner")}</div>
                    <div className="whitespace-pre-line text-secondary">{t("1. Click \"Add runner\", pick a project + pod and generate the CLI command.\n2. On the machine that will host the runner, run the displayed `pidash runner add` command. If the host is not logged in yet, the CLI starts `pidash auth login` first.\n3. The daemon registers the runner and it shows online here.\n\nPrerequisite: the agent CLI (codex / claude) must already be installed on the host.")}</div>
                  </div>
                }
              >
                <button
                  type="button"
                  aria-label={t("How to add a runner")}
                  className="text-tertiary hover:text-primary"
                >
                  <HelpCircle className="size-4" />
                </button>
              </Tooltip>
            </div>
            <div className="text-13 text-secondary">{t("`pidash runner add` starts `pidash auth login` first when the host is not logged in yet. Run it again for each project or pod this machine should serve.")}</div>
          </div>
          <Button onClick={() => setAddOpen(true)} disabled={!workspaceId}>
            {t("Add runner")}
          </Button>
        </div>
      </section>

      {/* Pods (read-only summary) */}
      <section>
        <div className="mb-2 text-13 font-medium text-primary">{t("Pods")}</div>
        <div className="mb-2 text-12 text-secondary">{t("Pods group your runners. Issues delegate to a pod, and any free runner inside picks up the work. Click a tile to filter runners.")}</div>
        {podsError ? (
          <div className="text-destructive text-12">{t("Failed to load pods")}</div>
        ) : (
          <div className="flex flex-wrap gap-2">
            {(pods ?? []).map((p) => {
              const isSelected = p.id === selectedPodId;
              return (
                <button
                  key={p.id}
                  type="button"
                  aria-pressed={isSelected}
                  aria-label={t("Filter runners by pod {name}", { name: p.name })}
                  onClick={() => setSelectedPodId(isSelected ? null : p.id)}
                  className={`rounded-md border px-3 py-2 text-left text-12 transition-colors ${
                    isSelected
                      ? "border-custom-primary-100 bg-custom-primary-100/10 ring-custom-primary-100 ring-1"
                      : "hover:border-primary border-subtle bg-layer-1"
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-primary">{p.name}</span>
                    {p.is_default && (
                      <Badge variant="accent-neutral" size="sm">
                        {t("default")}
                      </Badge>
                    )}
                  </div>
                  <div className="text-secondary">{t("{count} runner(s)", { count: p.runner_count })}</div>
                </button>
              );
            })}
            <button
              type="button"
              onClick={() => setCreatePodOpen(true)}
              disabled={!workspaceSlug}
              className="hover:border-primary flex items-center gap-1.5 rounded-md border border-dashed border-subtle bg-transparent px-3 py-2 text-12 text-secondary hover:text-primary disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Plus className="size-3.5" />
              <span className="font-medium">{t("Create new pod")}</span>
            </button>
          </div>
        )}
      </section>

      {/* Runners list — pending rows show as offline until the daemon enrolls */}
      <section>
        <div className="mb-2 flex items-center gap-3">
          <div className="text-13 font-medium text-primary">{t("Runners")}</div>
          {selectedPod && (
            <div className="flex items-center gap-2 text-12 text-secondary">
              <span>{t("Filtering runners by pod {name}", { name: selectedPod.name })}</span>
              <button
                type="button"
                onClick={() => setSelectedPodId(null)}
                className="text-custom-primary-100 underline-offset-2 hover:underline"
              >
                {t("Clear filter")}
              </button>
            </div>
          )}
        </div>
        <div className="overflow-x-auto rounded-md border border-subtle">
          <table className="w-full text-13">
            <thead className="bg-layer-1 text-left text-secondary">
              <tr>
                <th className="px-3 py-2">{t("Name")}</th>
                <th className="px-3 py-2">{t("Pod")}</th>
                <th className="px-3 py-2">{t("Status")}</th>
                <th className="px-3 py-2">{t("OS / Arch")}</th>
                <th className="px-3 py-2">{t("Version")}</th>
                <th className="px-3 py-2">{t("Last heartbeat")}</th>
                <th className="px-3 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {(filteredRunners ?? []).map((r) => (
                <tr key={r.id} className="border-t border-subtle">
                  <td className="font-mono px-3 py-2 text-11">{r.name}</td>
                  <td className="px-3 py-2">{r.pod_detail ? r.pod_detail.name : "—"}</td>
                  <td className="px-3 py-2">
                    <Badge variant={STATUS_BADGE_VARIANT[r.status]} size="sm">
                      {t(RUNNER_STATUS_I18N_LABELS[r.status])}
                    </Badge>
                  </td>
                  <td className="px-3 py-2">{r.os ? `${r.os} / ${r.arch}` : "—"}</td>
                  <td className="px-3 py-2">{r.runner_version || "—"}</td>
                  <td className="px-3 py-2">
                    {r.last_heartbeat_at ? new Date(r.last_heartbeat_at).toLocaleString() : "—"}
                  </td>
                  <td className="px-3 py-2 text-right">
                    <div className="flex justify-end gap-2">
                      {isRevocable(r) && (
                        <Button variant="outline-danger" size="sm" onClick={() => setRevokeRunner(r)}>
                          {t("Revoke")}
                        </Button>
                      )}
                      <Button variant="tertiary-danger" size="sm" onClick={() => setDeleteRunner(r)}>
                        {t("Delete")}
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
              {(filteredRunners ?? []).length === 0 && (
                <tr>
                  <td colSpan={7} className="px-3 py-8 text-center text-secondary">
                    {t("No runners yet. Click \"Add runner\" to generate your first runner command.")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>

      {workspaceId && workspaceSlug && (
        <AddRunnerModal
          isOpen={addOpen}
          onClose={() => setAddOpen(false)}
          workspaceId={workspaceId}
          workspaceSlug={workspaceSlug}
        />
      )}
      {workspaceSlug && (
        <CreatePodModal
          isOpen={createPodOpen}
          onClose={() => setCreatePodOpen(false)}
          workspaceSlug={workspaceSlug}
          onCreated={() => mutatePods()}
        />
      )}
      <AlertModalCore
        isOpen={!!deleteRunner}
        handleClose={() => (deleting ? null : setDeleteRunner(null))}
        handleSubmit={confirmDeleteRunner}
        isSubmitting={deleting}
        title={t("Delete runner?")}
        content={t("The runner row is removed and the daemon is forced offline. Historic runs are preserved with a null runner reference.")}
        primaryButtonText={{ default: t("Delete"), loading: t("Delete") }}
      />
      <AlertModalCore
        isOpen={!!revokeRunner}
        handleClose={() => (revoking ? null : setRevokeRunner(null))}
        handleSubmit={confirmRevokeRunner}
        isSubmitting={revoking}
        title={t("Revoke runner?")}
        content={t("The runner's credentials are invalidated and any in-flight runs are cancelled, but the row stays in the list. To attach it again, delete it and add a new runner from the target machine.")}
        primaryButtonText={{ default: t("Revoke"), loading: t("Revoke") }}
      />
    </div>
  );
});

export default RunnersListPage;
