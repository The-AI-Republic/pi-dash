/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { ArrowLeft, MessageSquare } from "lucide-react";
import { Link, useNavigate, useParams } from "react-router";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { getRunnerDetail } from "@pi-dash/services";
import type { IRunner } from "@pi-dash/types";
import { Badge, Button } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { RunnerAgentStatusPanel } from "@/components/runners/runner-agent-status-panel";
import { RUNNER_STATUS_I18N_LABELS, STATUS_BADGE_VARIANT } from "@/components/runners/runner-status";

function formatDateTime(ts: string | null): string {
  if (!ts) return "—";
  const d = new Date(ts);
  return Number.isNaN(d.getTime()) ? "—" : d.toLocaleString();
}

function MetaRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-secondary">{label}</dt>
      <dd className="min-w-0 break-words text-primary">{children}</dd>
    </>
  );
}

const RunnerDetailPage = observer(function RunnerDetailPage() {
  const { workspaceSlug, runnerId } = useParams<{ workspaceSlug: string; runnerId: string }>();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const base = `/${workspaceSlug}/runners`;

  const {
    data: runner,
    error,
    isLoading,
  } = useSWR<IRunner>(runnerId ? ["runner-detail", runnerId] : null, () => getRunnerDetail(runnerId!), {
    refreshInterval: 5_000,
  });

  return (
    <div className="flex flex-col gap-6">
      <PageHead title={runner?.name ? t("Runner {name}", { name: runner.name }) : t("Runner detail")} />

      <div className="flex items-center justify-between gap-3">
        <Link to={base} className="flex items-center gap-1.5 text-13 text-secondary hover:text-primary">
          <ArrowLeft className="size-4" />
          <span>{t("Back to runners")}</span>
        </Link>
        {runner && (
          <Button variant="neutral-primary" size="sm" onClick={() => navigate(`${base}/chat/${runner.id}`)}>
            <MessageSquare className="mr-1.5 size-4" />
            {t("Open chat")}
          </Button>
        )}
      </div>

      {error && !runner ? (
        <div className="rounded-md border border-subtle p-4 text-13 text-danger-primary">
          {t("Failed to load runner")}
        </div>
      ) : isLoading || !runner ? (
        <div className="rounded-md border border-subtle p-4 text-13 text-secondary">{t("Loading…")}</div>
      ) : (
        <>
          <section className="rounded-md border border-subtle p-4">
            <div className="flex items-center gap-3">
              <h1 className="font-mono text-15 font-semibold break-all text-primary">{runner.name}</h1>
              <Badge variant={STATUS_BADGE_VARIANT[runner.status]} size="sm">
                {t(RUNNER_STATUS_I18N_LABELS[runner.status])}
              </Badge>
            </div>
          </section>

          <section className="rounded-md border border-subtle p-4">
            <div className="mb-3 text-13 font-medium text-primary">{t("Metadata")}</div>
            <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-13">
              <MetaRow label={t("Runner ID")}>
                <span className="font-mono text-11">{runner.id}</span>
              </MetaRow>
              <MetaRow label={t("Pod")}>{runner.pod_detail ? runner.pod_detail.name : "—"}</MetaRow>
              <MetaRow label={t("Project")}>{runner.pod_detail?.project_identifier || "—"}</MetaRow>
              <MetaRow label={t("Dev machine")}>
                {runner.dev_machine_detail
                  ? runner.dev_machine_detail.label || runner.dev_machine_detail.host_label || "—"
                  : "—"}
              </MetaRow>
              <MetaRow label={t("OS / Arch")}>{runner.os ? `${runner.os} / ${runner.arch}` : "—"}</MetaRow>
              <MetaRow label={t("Version")}>{runner.runner_version || "—"}</MetaRow>
              <MetaRow label={t("Protocol version")}>{runner.protocol_version}</MetaRow>
              <MetaRow label={t("Capabilities")}>
                {runner.capabilities.length > 0 ? (
                  <div className="flex flex-wrap gap-1">
                    {runner.capabilities.map((cap) => (
                      <Badge key={cap} variant="accent-neutral" size="sm">
                        {cap}
                      </Badge>
                    ))}
                  </div>
                ) : (
                  "—"
                )}
              </MetaRow>
              <MetaRow label={t("Connection")}>
                <span className="font-mono text-11">{runner.connection || "—"}</span>
              </MetaRow>
              <MetaRow label={t("Owner")}>
                <span className="font-mono text-11">{runner.owner || "—"}</span>
              </MetaRow>
              <MetaRow label={t("Last heartbeat")}>{formatDateTime(runner.last_heartbeat_at)}</MetaRow>
              <MetaRow label={t("Enrolled at")}>
                {runner.enrolled_at ? formatDateTime(runner.enrolled_at) : t("Pending enrollment")}
              </MetaRow>
              {runner.revoked_at && <MetaRow label={t("Revoked at")}>{formatDateTime(runner.revoked_at)}</MetaRow>}
              {runner.revoked_reason && <MetaRow label={t("Revoked reason")}>{runner.revoked_reason}</MetaRow>}
              <MetaRow label={t("Created at")}>{formatDateTime(runner.created_at)}</MetaRow>
              <MetaRow label={t("Updated at")}>{formatDateTime(runner.updated_at)}</MetaRow>
            </dl>
          </section>

          <section>
            <div className="mb-2 text-13 font-medium text-primary">{t("Agent activity")}</div>
            <RunnerAgentStatusPanel runner={runner} liveState={runner.live_state} />
          </section>
        </>
      )}
    </div>
  );
});

export default RunnerDetailPage;
