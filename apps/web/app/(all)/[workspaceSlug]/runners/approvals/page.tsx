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
import type { IApprovalRequest, TApprovalDecision, TApprovalKind } from "@pi-dash/types";
import { Button } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { RunnersTabs } from "@/components/runners/runners-tabs";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();

const APPROVAL_KIND_I18N_LABELS: Record<TApprovalKind | "other", string> = {
  command_execution: "The runner wants to run a shell command",
  file_change: "The runner wants to modify a file",
  network_access: "The runner wants to make a network call",
  other: "The runner is requesting approval",
};

function approvalKindI18nLabel(kind: TApprovalKind): string {
  return APPROVAL_KIND_I18N_LABELS[kind] ?? APPROVAL_KIND_I18N_LABELS.other;
}

export const ApprovalsPage = observer(function ApprovalsPage() {
  const { t } = useTranslation();
  const { currentWorkspace } = useWorkspace();
  const { data: approvals, mutate } = useSWR<IApprovalRequest[]>("runner-approvals", () => service.listApprovals(), {
    refreshInterval: 2_000,
  });
  const [pending, setPending] = useState<string | null>(null);
  const pageTitle = currentWorkspace?.name
    ? t("{workspace} - AI Agents", { workspace: currentWorkspace.name })
    : t("AI Agents");

  async function decide(id: string, decision: TApprovalDecision) {
    setPending(id);
    try {
      await service.decideApproval(id, decision);
      mutate();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message: err?.error ?? t("Failed to record decision"),
      });
    } finally {
      setPending(null);
    }
  }

  const rows = approvals ?? [];

  return (
    <div className="flex flex-col gap-6">
      <PageHead title={pageTitle} />
      <RunnersTabs />
      {rows.length === 0 ? (
        <div className="rounded-md border border-subtle p-8 text-center text-13 text-secondary">
          {t("No pending approvals.")}
        </div>
      ) : (
        <div className="flex flex-col gap-4">
          {rows.map((a) => (
            <div key={a.id} className="rounded-md border border-subtle p-4">
              <div className="flex items-start justify-between">
                <div>
                  <div className="text-11 text-secondary">
                    {t("Run {runId} · requested {at}", {
                      runId: a.agent_run,
                      at: new Date(a.requested_at).toLocaleTimeString(),
                    })}
                  </div>
                  <div className="text-13 font-medium text-primary">{t(approvalKindI18nLabel(a.kind))}</div>
                  {a.reason && <div className="mt-1 text-13 text-secondary">{a.reason}</div>}
                </div>
                {a.expires_at && (
                  <div className="text-11 text-secondary">
                    {t("expires {at}", { at: new Date(a.expires_at).toLocaleTimeString() })}
                  </div>
                )}
              </div>
              <pre className="font-mono mt-3 max-h-80 overflow-auto rounded bg-layer-1 p-3 text-11 whitespace-pre-wrap">
                {JSON.stringify(a.payload, null, 2)}
              </pre>
              <div className="mt-3 flex gap-2">
                <Button onClick={() => decide(a.id, "accept")} loading={pending === a.id} size="sm">
                  {t("Accept once")}
                </Button>
                <Button
                  onClick={() => decide(a.id, "accept_for_session")}
                  loading={pending === a.id}
                  variant="accent-primary"
                  size="sm"
                >
                  {t("Accept for session")}
                </Button>
                <Button
                  onClick={() => decide(a.id, "decline")}
                  loading={pending === a.id}
                  variant="outline-danger"
                  size="sm"
                >
                  {t("Decline")}
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
});

export default ApprovalsPage;
