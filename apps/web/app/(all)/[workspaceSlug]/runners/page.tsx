/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { observer } from "mobx-react";
import { HelpCircle } from "lucide-react";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { PodService, RunnerService } from "@pi-dash/services";
import type { IPod, IRunner, TRunnerStatus } from "@pi-dash/types";
import type { TBadgeVariant } from "@pi-dash/ui";
import { AlertModalCore, Badge, Button, Checkbox, Tooltip } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { AddRunnerModal } from "@/components/runners/add-runner-modal";
import { RunnerAgentStatusPanel } from "@/components/runners/runner-agent-status-panel";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();
const podService = new PodService();

const STATUS_BADGE_VARIANT: Record<TRunnerStatus, TBadgeVariant> = {
  online: "accent-success",
  busy: "accent-primary",
  offline: "accent-neutral",
  revoked: "accent-warning",
};

const RunnersListPage = observer(function RunnersListPage() {
  const { currentWorkspace } = useWorkspace();
  const { t } = useTranslation();
  const workspaceId = currentWorkspace?.id;
  const workspaceSlug = currentWorkspace?.slug;
  const pageTitle = currentWorkspace?.name
    ? t("runners.page_title", { workspace: currentWorkspace.name })
    : t("runners.title");

  const { data: runners, mutate: mutateRunners } = useSWR<IRunner[]>(
    workspaceId ? ["runners", workspaceId] : null,
    () => service.list(workspaceId),
    { refreshInterval: 5_000 }
  );

  const { data: pods, error: podsError } = useSWR<IPod[]>(
    workspaceId ? ["pods", workspaceId] : null,
    () => podService.list(workspaceId!),
    { refreshInterval: 30_000 }
  );

  const [addOpen, setAddOpen] = useState(false);
  const [deleteRunner, setDeleteRunner] = useState<IRunner | null>(null);
  const [deleting, setDeleting] = useState(false);
  // Default-checked per spec: when the runner is connected, the
  // "Also delete the local runner instance" cascade is the action a
  // user clicking through the dialog usually wants. Reset to true on
  // every modal open so a previous "Cancel" doesn't carry an
  // unchecked state into the next attempt.
  const [purgeLocal, setPurgeLocal] = useState(true);
  useEffect(() => {
    if (deleteRunner) setPurgeLocal(true);
  }, [deleteRunner]);

  // The cascade self-uninstall only works while the daemon is alive
  // to receive the `remove_runner` frame. ``busy`` counts as
  // connected — the daemon is in the middle of a run but is still
  // polling the session. ``offline`` and ``revoked`` cannot cascade.
  const isConnected = deleteRunner?.status === "online" || deleteRunner?.status === "busy";
  // Force purge_local off when the runner is disconnected — there's
  // nothing for the daemon to act on. The modal hides the checkbox
  // in that case and shows the offline notice instead.
  const effectivePurgeLocal = isConnected && purgeLocal;

  async function confirmDeleteRunner() {
    if (!deleteRunner) return;
    setDeleting(true);
    try {
      await service.deleteRunner(deleteRunner.id, { purgeLocal: effectivePurgeLocal });
      setDeleteRunner(null);
      mutateRunners();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("runners.list.delete_failed"),
      });
    } finally {
      setDeleting(false);
    }
  }

  const deleteModalContent = deleteRunner ? (
    <div className="flex flex-col gap-3">
      <div>{t("runners.list.delete_confirm_body")}</div>
      {isConnected ? (
        <label className="flex cursor-pointer items-start gap-2 text-13 text-secondary">
          <Checkbox
            checked={purgeLocal}
            onChange={(e) => setPurgeLocal(e.target.checked)}
            disabled={deleting}
            className="mt-0.5"
          />
          <span>{t("runners.list.delete_purge_local_label")}</span>
        </label>
      ) : (
        <div className="rounded-md border border-subtle bg-layer-1 px-3 py-2 text-12 text-secondary">
          {t("runners.list.delete_offline_notice")}
        </div>
      )}
    </div>
  ) : null;

  return (
    <div className="flex flex-col gap-6">
      <PageHead title={pageTitle} />

      {/* Header — primary "Add runner" CTA + how-it-works tooltip */}
      <section className="rounded-md border border-subtle p-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-1.5">
              <div className="text-13 font-medium text-primary">{t("runners.list.add_runner")}</div>
              <Tooltip
                position="bottom"
                tooltipContent={
                  <div className="flex max-w-xs flex-col gap-1 p-1 text-12 whitespace-normal">
                    <div className="font-medium">{t("runners.list.how_it_works_title")}</div>
                    <div className="whitespace-pre-line text-secondary">{t("runners.list.how_it_works_body")}</div>
                  </div>
                }
              >
                <button
                  type="button"
                  aria-label={t("runners.list.how_it_works_title")}
                  className="text-tertiary hover:text-primary"
                >
                  <HelpCircle className="size-4" />
                </button>
              </Tooltip>
            </div>
            <div className="text-13 text-secondary">{t("runners.machine_token_note.body")}</div>
          </div>
          <Button onClick={() => setAddOpen(true)} disabled={!workspaceId}>
            {t("runners.list.add_runner")}
          </Button>
        </div>
      </section>

      {/* Pods (read-only summary) */}
      <section>
        <div className="mb-2 text-13 font-medium text-primary">{t("runners.pods.title")}</div>
        <div className="mb-2 text-12 text-secondary">{t("runners.pods.help")}</div>
        {podsError ? (
          <div className="text-destructive text-12">{t("runners.pods.load_failed")}</div>
        ) : (
          <div className="flex flex-wrap gap-2">
            {(pods ?? []).map((p) => (
              <div key={p.id} className="rounded-md border border-subtle bg-layer-1 px-3 py-2 text-12">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-primary">{p.name}</span>
                  {p.is_default && (
                    <Badge variant="accent-neutral" size="sm">
                      {t("runners.pods.default_badge")}
                    </Badge>
                  )}
                </div>
                <div className="text-secondary">{t("runners.pods.runner_count", { count: p.runner_count })}</div>
              </div>
            ))}
            {(pods ?? []).length === 0 && <div className="text-12 text-secondary">{t("runners.pods.empty")}</div>}
          </div>
        )}
      </section>

      {/* Runners list — pending rows show as offline until the daemon enrolls */}
      <section>
        <div className="mb-2 text-13 font-medium text-primary">{t("runners.list.connected_runners")}</div>
        <div className="overflow-x-auto rounded-md border border-subtle">
          <table className="w-full text-13">
            <thead className="bg-layer-1 text-left text-secondary">
              <tr>
                <th className="px-3 py-2">{t("runners.list.columns.name")}</th>
                <th className="px-3 py-2">{t("runners.list.columns_pod")}</th>
                <th className="px-3 py-2">{t("runners.list.columns.status")}</th>
                <th className="px-3 py-2">{t("runners.list.columns.os_arch")}</th>
                <th className="px-3 py-2">{t("runners.list.columns.version")}</th>
                <th className="px-3 py-2">{t("runners.list.columns.last_heartbeat")}</th>
                <th className="px-3 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {(runners ?? []).map((r) => (
                <tr key={r.id} className="border-t border-subtle">
                  <td className="font-mono px-3 py-2 text-11">{r.name}</td>
                  <td className="px-3 py-2">{r.pod_detail ? r.pod_detail.name : "—"}</td>
                  <td className="px-3 py-2">
                    <Badge variant={STATUS_BADGE_VARIANT[r.status]} size="sm">
                      {t(`runners.list.status.${r.status}`)}
                    </Badge>
                  </td>
                  <td className="px-3 py-2">{r.os ? `${r.os} / ${r.arch}` : "—"}</td>
                  <td className="px-3 py-2">{r.runner_version || "—"}</td>
                  <td className="px-3 py-2">
                    {r.last_heartbeat_at ? new Date(r.last_heartbeat_at).toLocaleString() : "—"}
                  </td>
                  <td className="px-3 py-2 text-right">
                    <Button variant="tertiary-danger" size="sm" onClick={() => setDeleteRunner(r)}>
                      {t("runners.list.delete")}
                    </Button>
                  </td>
                </tr>
              ))}
              {(runners ?? []).length === 0 && (
                <tr>
                  <td colSpan={7} className="px-3 py-8 text-center text-secondary">
                    {t("runners.list.empty")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>

      {/* Active-run observability: render one panel per runner that
          currently reports an in-flight ``observed_run_id``. Lets
          operators see which agent is doing what without clicking
          through to a per-runner detail page. */}
      {(runners ?? [])
        .filter((r) => r.live_state?.observed_run_id)
        .map((r) => (
          <RunnerAgentStatusPanel
            key={`agent-status-${r.id}`}
            runner={{ id: r.id, name: r.name, status: r.status }}
            liveState={r.live_state}
          />
        ))}

      {workspaceId && workspaceSlug && (
        <AddRunnerModal
          isOpen={addOpen}
          onClose={() => setAddOpen(false)}
          workspaceId={workspaceId}
          workspaceSlug={workspaceSlug}
          onCreated={() => mutateRunners()}
        />
      )}
      <AlertModalCore
        isOpen={!!deleteRunner}
        handleClose={() => (deleting ? null : setDeleteRunner(null))}
        handleSubmit={confirmDeleteRunner}
        isSubmitting={deleting}
        title={t("runners.list.delete_confirm_title")}
        content={deleteModalContent}
        primaryButtonText={{ default: t("runners.list.delete"), loading: t("runners.list.delete") }}
      />
    </div>
  );
});

export default RunnersListPage;
