/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
import { useTranslation } from "@apple-pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@apple-pi-dash/propel/toast";
import { RunnerService } from "@apple-pi-dash/services";
import type { IRunner, TRunnerStatus } from "@apple-pi-dash/types";
import type { TBadgeVariant } from "@apple-pi-dash/ui";
import { AlertModalCore, Badge, Button, Input } from "@apple-pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();

const MAX_RUNNERS_PER_USER = 5;

const STATUS_BADGE_VARIANT: Record<TRunnerStatus, TBadgeVariant> = {
  online: "accent-success",
  busy: "accent-primary",
  offline: "accent-neutral",
  revoked: "accent-destructive",
};

const RunnersListPage = observer(function RunnersListPage() {
  const { currentWorkspace } = useWorkspace();
  const { t } = useTranslation();
  const workspaceId = currentWorkspace?.id;
  const pageTitle = currentWorkspace?.name
    ? t("runners.page_title", { workspace: currentWorkspace.name })
    : t("runners.title");

  const { data: runners, mutate } = useSWR<IRunner[]>(
    workspaceId ? ["runners", workspaceId] : null,
    () => service.list(workspaceId),
    { refreshInterval: 5_000 }
  );

  const [mintedToken, setMintedToken] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [minting, setMinting] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<IRunner | null>(null);
  const [revoking, setRevoking] = useState(false);
  const [origin, setOrigin] = useState("");

  useEffect(() => {
    if (typeof window !== "undefined") setOrigin(window.location.origin);
  }, []);

  const activeCount = (runners ?? []).filter((r) => r.status !== "revoked").length;
  const atCap = activeCount >= MAX_RUNNERS_PER_USER;

  async function mint() {
    if (!workspaceId) return;
    setMinting(true);
    try {
      const result = await service.mintToken(workspaceId, label || undefined);
      setMintedToken(result.token);
      setLabel("");
      mutate();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("runners.list.mint_failed"),
      });
    } finally {
      setMinting(false);
    }
  }

  async function confirmRevoke() {
    if (!revokeTarget) return;
    setRevoking(true);
    try {
      await service.revoke(revokeTarget.id);
      setRevokeTarget(null);
      mutate();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("runners.list.revoke_failed"),
      });
    } finally {
      setRevoking(false);
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <PageHead title={pageTitle} />
      <section className="rounded-md border border-subtle p-4">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-13 font-medium text-primary">{t("runners.list.add_runner")}</div>
            <div className="text-13 text-secondary">
              {t("runners.list.cap_count", { active: activeCount, max: MAX_RUNNERS_PER_USER })}
            </div>
          </div>
        </div>
        <div className="mt-3 flex items-center gap-2">
          <Input
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder={t("runners.list.label_placeholder")}
            className="flex-1"
          />
          <Button onClick={mint} disabled={!workspaceId || atCap} loading={minting}>
            {atCap ? t("runners.list.cap_reached") : t("runners.list.mint")}
          </Button>
        </div>
        {mintedToken && (
          <div className="border-amber-300 bg-amber-50 mt-3 rounded border p-3 text-13 text-primary">
            <div className="font-medium">{t("runners.list.token_warning")}</div>
            <pre className="font-mono mt-2 text-11 break-all select-all">{mintedToken}</pre>
            <div className="mt-2 text-secondary">
              {t("runners.list.token_run_instructions")}
              <pre className="font-mono mt-1 text-11 whitespace-pre-wrap select-all">
                apple-pi-dash-runner configure --url {origin} --token {mintedToken}
              </pre>
            </div>
            <div className="mt-3">
              <Button variant="outline-primary" size="sm" onClick={() => setMintedToken(null)}>
                {t("runners.list.dismiss_token")}
              </Button>
            </div>
          </div>
        )}
      </section>

      <section>
        <div className="mb-2 text-13 font-medium text-primary">{t("runners.list.connected_runners")}</div>
        <div className="overflow-x-auto rounded-md border border-subtle">
          <table className="w-full text-13">
            <thead className="bg-layer-1 text-left text-secondary">
              <tr>
                <th className="px-3 py-2">{t("runners.list.columns.name")}</th>
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
                  <td className="px-3 py-2">
                    <Badge variant={STATUS_BADGE_VARIANT[r.status]} size="sm">
                      {t(`runners.list.status.${r.status}`)}
                    </Badge>
                  </td>
                  <td className="px-3 py-2">
                    {r.os} / {r.arch}
                  </td>
                  <td className="px-3 py-2">{r.runner_version || "—"}</td>
                  <td className="px-3 py-2">
                    {r.last_heartbeat_at ? new Date(r.last_heartbeat_at).toLocaleString() : "—"}
                  </td>
                  <td className="px-3 py-2 text-right">
                    {r.status !== "revoked" && (
                      <Button variant="tertiary-danger" size="sm" onClick={() => setRevokeTarget(r)}>
                        {t("runners.list.revoke")}
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
              {(runners ?? []).length === 0 && (
                <tr>
                  <td colSpan={6} className="px-3 py-8 text-center text-secondary">
                    {t("runners.list.empty")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>

      <AlertModalCore
        isOpen={!!revokeTarget}
        handleClose={() => (revoking ? null : setRevokeTarget(null))}
        handleSubmit={confirmRevoke}
        isSubmitting={revoking}
        title={t("runners.list.revoke_confirm_title")}
        content={t("runners.list.revoke_confirm_body")}
        primaryButtonText={{ default: t("runners.list.revoke"), loading: t("runners.list.revoke") }}
      />
    </div>
  );
});

export default RunnersListPage;
