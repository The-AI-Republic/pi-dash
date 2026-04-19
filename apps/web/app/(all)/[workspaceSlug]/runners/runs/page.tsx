/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { RunnerService } from "@pi-dash/services";
import type { IAgentRun, TAgentRunStatus } from "@pi-dash/types";
import { AGENT_RUN_TERMINAL_STATUSES } from "@pi-dash/types";
import type { TBadgeVariant } from "@pi-dash/ui";
import { AlertModalCore, Badge, Button } from "@pi-dash/ui";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();

const STATUS_BADGE_VARIANT: Record<TAgentRunStatus, TBadgeVariant> = {
  queued: "accent-neutral",
  assigned: "accent-primary",
  running: "primary",
  awaiting_approval: "accent-warning",
  awaiting_reauth: "accent-warning",
  completed: "accent-success",
  failed: "accent-destructive",
  cancelled: "accent-neutral",
};

function isTerminal(status: TAgentRunStatus): boolean {
  return AGENT_RUN_TERMINAL_STATUSES.includes(status);
}

const RunnerRunsPage = observer(function RunnerRunsPage() {
  const { currentWorkspace } = useWorkspace();
  const { t } = useTranslation();
  const workspaceId = currentWorkspace?.id;

  const { data: runs, mutate } = useSWR<IAgentRun[]>(
    workspaceId ? ["runner-runs", workspaceId] : null,
    () => service.listRuns(workspaceId),
    { refreshInterval: 5_000 }
  );

  const [selected, setSelected] = useState<string | null>(null);
  const { data: detail } = useSWR<IAgentRun>(
    selected ? ["runner-run-detail", selected] : null,
    () => service.getRun(selected!, true),
    {
      refreshInterval: (latest) => (latest && isTerminal(latest.status) ? 0 : 3_000),
    }
  );

  const [cancelTarget, setCancelTarget] = useState<IAgentRun | null>(null);
  const [cancelling, setCancelling] = useState(false);

  async function confirmCancel() {
    if (!cancelTarget) return;
    setCancelling(true);
    try {
      await service.cancelRun(cancelTarget.id, "user");
      setCancelTarget(null);
      mutate();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("runners.runs.cancel_failed"),
      });
    } finally {
      setCancelling(false);
    }
  }

  return (
    <div className="grid grid-cols-[400px_1fr] gap-4">
      <div className="rounded-md border border-subtle">
        <table className="w-full text-13">
          <thead className="bg-layer-1 text-left text-secondary">
            <tr>
              <th className="px-3 py-2">{t("runners.runs.columns.started")}</th>
              <th className="px-3 py-2">{t("runners.runs.columns.status")}</th>
              <th className="px-3 py-2">{t("runners.runs.columns.prompt")}</th>
            </tr>
          </thead>
          <tbody>
            {(runs ?? []).map((r) => (
              <tr
                key={r.id}
                onClick={() => setSelected(r.id)}
                className={`cursor-pointer border-t border-subtle ${
                  selected === r.id ? "bg-accent-subtle" : "hover:bg-layer-1"
                }`}
              >
                <td className="px-3 py-2 whitespace-nowrap">{new Date(r.created_at).toLocaleString()}</td>
                <td className="px-3 py-2">
                  <Badge variant={STATUS_BADGE_VARIANT[r.status]} size="sm">
                    {t(`runners.runs.status.${r.status}`)}
                  </Badge>
                </td>
                <td className="font-mono max-w-[180px] truncate px-3 py-2 text-11">{r.prompt}</td>
              </tr>
            ))}
            {(runs ?? []).length === 0 && (
              <tr>
                <td colSpan={3} className="px-3 py-8 text-center text-secondary">
                  {t("runners.runs.empty")}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <div className="rounded-md border border-subtle p-4">
        {!detail ? (
          <div className="text-13 text-secondary">{t("runners.runs.select_run")}</div>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <div>
                <div className="font-mono text-11">{detail.id}</div>
                <div className="mt-1">
                  <Badge variant={STATUS_BADGE_VARIANT[detail.status]} size="sm">
                    {t(`runners.runs.status.${detail.status}`)}
                  </Badge>
                </div>
              </div>
              {!isTerminal(detail.status) && (
                <Button variant="tertiary-danger" size="sm" onClick={() => setCancelTarget(detail)}>
                  {t("runners.runs.cancel")}
                </Button>
              )}
            </div>
            <div className="text-13">
              <div className="text-secondary">{t("runners.runs.prompt")}</div>
              <pre className="mt-1 rounded bg-layer-1 p-2 text-11 whitespace-pre-wrap">{detail.prompt}</pre>
            </div>
            {detail.error && (
              <div className="text-13">
                <div className="text-danger-primary">{t("runners.runs.error")}</div>
                <pre className="mt-1 rounded bg-danger-subtle p-2 text-11 whitespace-pre-wrap">{detail.error}</pre>
              </div>
            )}
            {detail.done_payload && (
              <div className="text-13">
                <div className="text-secondary">{t("runners.runs.done_payload")}</div>
                <pre className="mt-1 rounded bg-layer-1 p-2 text-11 whitespace-pre-wrap">
                  {JSON.stringify(detail.done_payload, null, 2)}
                </pre>
              </div>
            )}
            <div className="text-13">
              <div className="text-secondary">
                {t("runners.runs.events_count", { count: detail.events?.length ?? 0 })}
              </div>
              <div className="mt-1 max-h-[420px] overflow-auto rounded border border-subtle">
                <table className="w-full text-11">
                  <thead className="bg-layer-1 text-left text-secondary">
                    <tr>
                      <th className="px-2 py-1">{t("runners.runs.event_columns.seq")}</th>
                      <th className="px-2 py-1">{t("runners.runs.event_columns.kind")}</th>
                      <th className="px-2 py-1">{t("runners.runs.event_columns.at")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(detail.events ?? []).map((e) => (
                      <tr key={e.id} className="border-t border-subtle">
                        <td className="font-mono px-2 py-1">{e.seq}</td>
                        <td className="font-mono px-2 py-1">{e.kind}</td>
                        <td className="px-2 py-1">{new Date(e.created_at).toLocaleTimeString()}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        )}
      </div>

      <AlertModalCore
        isOpen={!!cancelTarget}
        handleClose={() => (cancelling ? null : setCancelTarget(null))}
        handleSubmit={confirmCancel}
        isSubmitting={cancelling}
        title={t("runners.runs.cancel_confirm_title")}
        content={t("runners.runs.cancel_confirm_body")}
        primaryButtonText={{ default: t("runners.runs.cancel"), loading: t("runners.runs.cancel") }}
      />
    </div>
  );
});

export default RunnerRunsPage;
