/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { useNavigate, useParams } from "react-router";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { RunnerService } from "@pi-dash/services";
import type { IAgentRun, TAgentRunErrorSource, TAgentRunStatus } from "@pi-dash/types";
import { AGENT_RUN_TERMINAL_STATUSES } from "@pi-dash/types";
import type { TBadgeVariant } from "@pi-dash/ui";
import { AlertModalCore, Badge, Button, Spinner } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { RunnersTabs } from "@/components/runners/runners-tabs";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();

const STATUS_BADGE_VARIANT: Record<TAgentRunStatus, TBadgeVariant> = {
  queued: "accent-neutral",
  assigned: "accent-primary",
  waiting_for_worktree: "accent-primary",
  running: "primary",
  awaiting_approval: "accent-warning",
  awaiting_reauth: "accent-warning",
  paused_awaiting_input: "accent-warning",
  blocked: "accent-warning",
  completed: "accent-success",
  failed: "accent-destructive",
  cancelled: "accent-neutral",
};

const RUN_STATUS_I18N_LABELS: Record<TAgentRunStatus, string> = {
  queued: "queued",
  assigned: "assigned",
  waiting_for_worktree: "waiting for worktree",
  running: "running",
  awaiting_approval: "awaiting approval",
  awaiting_reauth: "awaiting reauth",
  paused_awaiting_input: "paused awaiting input",
  blocked: "blocked",
  completed: "completed",
  failed: "failed",
  cancelled: "cancelled",
};

const ERROR_SOURCE_BADGE_VARIANT: Record<TAgentRunErrorSource, TBadgeVariant> = {
  agent: "accent-warning",
  pidash_runner: "accent-primary",
  pidash_cloud: "accent-primary",
  unknown: "accent-neutral",
};

function isTerminal(status: TAgentRunStatus): boolean {
  return AGENT_RUN_TERMINAL_STATUSES.includes(status);
}

export const RunnerRunsPage = observer(function RunnerRunsPage() {
  const { currentWorkspace } = useWorkspace();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { workspaceSlug, runId } = useParams<{ workspaceSlug: string; runId?: string }>();
  const workspaceId = currentWorkspace?.id;
  const selected = runId ?? null;

  const { data: runs, mutate } = useSWR<IAgentRun[]>(
    workspaceId ? ["runner-runs", workspaceId] : null,
    () => service.listRuns(workspaceId),
    { refreshInterval: 5_000 }
  );

  const { data: detail, error: detailError } = useSWR<IAgentRun>(
    selected ? ["runner-run-detail", selected] : null,
    () => service.getRun(selected!, true),
    {
      refreshInterval: (latest) => (latest && isTerminal(latest.status) ? 0 : 3_000),
      shouldRetryOnError: false,
    }
  );

  const [cancelTarget, setCancelTarget] = useState<IAgentRun | null>(null);
  const [cancelling, setCancelling] = useState(false);

  function selectRun(id: string) {
    if (!workspaceSlug) return;
    navigate(`/${workspaceSlug}/runners/runs/${id}`, { replace: true });
  }

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
        title: t("Error!"),
        message: err?.error ?? t("Failed to cancel run"),
      });
    } finally {
      setCancelling(false);
    }
  }

  const pageTitle = currentWorkspace?.name
    ? t("{workspace} - AI Agents", { workspace: currentWorkspace.name })
    : t("AI Agents");

  return (
    <div className="flex h-full min-h-0 flex-col gap-6">
      <PageHead title={pageTitle} />
      <RunnersTabs />
      <div className="grid min-h-0 flex-1 grid-cols-[400px_1fr] gap-4 overflow-hidden">
        <div className="overflow-auto rounded-md border border-subtle">
          <table className="w-full text-13">
            <thead className="sticky top-0 z-10 bg-layer-1 text-left text-secondary">
              <tr>
                <th className="px-3 py-2">{t("Started")}</th>
                <th className="px-3 py-2">{t("Status")}</th>
                <th className="px-3 py-2">{t("Prompt")}</th>
              </tr>
            </thead>
            <tbody>
              {(runs ?? []).map((r) => (
                <tr
                  key={r.id}
                  onClick={() => selectRun(r.id)}
                  className={`cursor-pointer border-t border-subtle ${
                    selected === r.id ? "bg-accent-subtle" : "hover:bg-layer-1"
                  }`}
                >
                  <td className="px-3 py-2 whitespace-nowrap">{new Date(r.created_at).toLocaleString()}</td>
                  <td className="px-3 py-2">
                    <Badge variant={STATUS_BADGE_VARIANT[r.status]} size="sm">
                      {t(RUN_STATUS_I18N_LABELS[r.status])}
                    </Badge>
                  </td>
                  <td className="font-mono max-w-[180px] truncate px-3 py-2 text-11">{r.prompt}</td>
                </tr>
              ))}
              {(runs ?? []).length === 0 && (
                <tr>
                  <td colSpan={3} className="px-3 py-8 text-center text-secondary">
                    {t("No runs yet.")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        <div className="overflow-auto rounded-md border border-subtle p-4">
          {selected && detailError ? (
            <div className="text-13 text-danger-primary">
              {t("This run is not available. It may have been deleted or belong to a different workspace.")}
            </div>
          ) : !selected ? (
            <div className="text-13 text-secondary">{t("Select a run on the left.")}</div>
          ) : !detail ? (
            <div className="flex items-center gap-2 text-13 text-secondary">
              <Spinner height="16px" width="16px" />
              {t("Loading run details…")}
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              <div className="flex items-center justify-between">
                <div>
                  <div className="font-mono text-11">{detail.id}</div>
                  <div className="mt-1 flex items-center gap-2">
                    <Badge variant={STATUS_BADGE_VARIANT[detail.status]} size="sm">
                      {t(RUN_STATUS_I18N_LABELS[detail.status])}
                    </Badge>
                    {detail.status === "waiting_for_worktree" &&
                      typeof detail.queue_position === "number" &&
                      detail.queue_position > 0 && (
                        <span className="text-11 text-secondary">
                          {t("Queued (position {count})", { count: detail.queue_position })}
                        </span>
                      )}
                  </div>
                </div>
                {!isTerminal(detail.status) && (
                  <Button variant="tertiary-danger" size="sm" onClick={() => setCancelTarget(detail)}>
                    {t("Cancel run")}
                  </Button>
                )}
              </div>
              <div className="text-13">
                <div className="text-secondary">{t("Prompt")}</div>
                <pre className="mt-1 rounded bg-layer-1 p-2 text-11 whitespace-pre-wrap">{detail.prompt}</pre>
              </div>
              {detail.error && (
                <div className="text-13">
                  <div className="text-danger-primary">{t("Error")}</div>
                  {detail.error_diagnostic && (
                    <div className="mt-1 rounded border border-warning-subtle bg-layer-1 p-3">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="text-secondary">{t("Failure source")}</span>
                        <Badge
                          variant={ERROR_SOURCE_BADGE_VARIANT[detail.error_diagnostic.source]}
                          size="sm"
                        >
                          {detail.error_diagnostic.source_label}
                        </Badge>
                        <span className="font-mono text-11 text-secondary">{detail.error_diagnostic.kind}</span>
                      </div>
                      <div className="mt-2 text-primary">{detail.error_diagnostic.summary}</div>
                      {detail.error_diagnostic.action && (
                        <div className="mt-1 text-secondary">{detail.error_diagnostic.action}</div>
                      )}
                    </div>
                  )}
                  <pre className="mt-1 rounded bg-danger-subtle p-2 text-11 whitespace-pre-wrap">{detail.error}</pre>
                </div>
              )}
              {detail.done_payload && (
                <div className="text-13">
                  <div className="text-secondary">{t("Done payload")}</div>
                  <pre className="mt-1 rounded bg-layer-1 p-2 text-11 whitespace-pre-wrap">
                    {JSON.stringify(detail.done_payload, null, 2)}
                  </pre>
                </div>
              )}
              <div className="text-13">
                <div className="text-secondary">{t("Events ({count})", { count: detail.events?.length ?? 0 })}</div>
                <div className="mt-1 max-h-[420px] overflow-auto rounded border border-subtle">
                  <table className="w-full text-11">
                    <thead className="bg-layer-1 text-left text-secondary">
                      <tr>
                        <th className="px-2 py-1">{t("seq")}</th>
                        <th className="px-2 py-1">{t("kind")}</th>
                        <th className="px-2 py-1">{t("at")}</th>
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
      </div>

      <AlertModalCore
        isOpen={!!cancelTarget}
        handleClose={() => (cancelling ? null : setCancelTarget(null))}
        handleSubmit={confirmCancel}
        isSubmitting={cancelling}
        title={t("Cancel run?")}
        content={t("The runner will stop this run as soon as it gets the signal.")}
        primaryButtonText={{ default: t("Cancel run"), loading: t("Cancel run") }}
      />
    </div>
  );
});

export default RunnerRunsPage;
