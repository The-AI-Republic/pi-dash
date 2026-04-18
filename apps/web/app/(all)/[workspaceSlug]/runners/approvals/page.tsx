/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
import { useTranslation } from "@apple-pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@apple-pi-dash/propel/toast";
import { RunnerService } from "@apple-pi-dash/services";
import type { IApprovalRequest, TApprovalDecision, TApprovalKind } from "@apple-pi-dash/types";
import { Button } from "@apple-pi-dash/ui";

const service = new RunnerService();

function kindKey(kind: TApprovalKind): string {
  switch (kind) {
    case "command_execution":
    case "file_change":
    case "network_access":
      return `runners.approvals.kinds.${kind}`;
    default:
      return "runners.approvals.kinds.other";
  }
}

const ApprovalsPage = observer(function ApprovalsPage() {
  const { t } = useTranslation();
  const { data: approvals, mutate } = useSWR<IApprovalRequest[]>("runner-approvals", () => service.listApprovals(), {
    refreshInterval: 2_000,
  });
  const [pending, setPending] = useState<string | null>(null);

  async function decide(id: string, decision: TApprovalDecision) {
    setPending(id);
    try {
      await service.decideApproval(id, decision);
      mutate();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("runners.approvals.decision_failed"),
      });
    } finally {
      setPending(null);
    }
  }

  const rows = approvals ?? [];
  if (rows.length === 0) {
    return (
      <div className="rounded-md border border-subtle p-8 text-center text-13 text-secondary">
        {t("runners.approvals.empty")}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      {rows.map((a) => (
        <div key={a.id} className="rounded-md border border-subtle p-4">
          <div className="flex items-start justify-between">
            <div>
              <div className="text-11 text-secondary">
                {t("runners.approvals.run_meta", {
                  runId: a.agent_run,
                  at: new Date(a.requested_at).toLocaleTimeString(),
                })}
              </div>
              <div className="text-13 font-medium text-primary">{t(kindKey(a.kind))}</div>
              {a.reason && <div className="mt-1 text-13 text-secondary">{a.reason}</div>}
            </div>
            {a.expires_at && (
              <div className="text-11 text-secondary">
                {t("runners.approvals.expires", { at: new Date(a.expires_at).toLocaleTimeString() })}
              </div>
            )}
          </div>
          <pre className="font-mono mt-3 max-h-80 overflow-auto rounded bg-layer-1 p-3 text-11 whitespace-pre-wrap">
            {JSON.stringify(a.payload, null, 2)}
          </pre>
          <div className="mt-3 flex gap-2">
            <Button onClick={() => decide(a.id, "accept")} loading={pending === a.id} size="sm">
              {t("runners.approvals.accept_once")}
            </Button>
            <Button
              onClick={() => decide(a.id, "accept_for_session")}
              loading={pending === a.id}
              variant="accent-primary"
              size="sm"
            >
              {t("runners.approvals.accept_for_session")}
            </Button>
            <Button
              onClick={() => decide(a.id, "decline")}
              loading={pending === a.id}
              variant="outline-danger"
              size="sm"
            >
              {t("runners.approvals.decline")}
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
});

export default ApprovalsPage;
