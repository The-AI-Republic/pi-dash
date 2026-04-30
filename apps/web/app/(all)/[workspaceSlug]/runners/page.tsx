/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { observer } from "mobx-react";
import { HelpCircle } from "lucide-react";
import useSWR from "swr";
import { API_BASE_URL } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { PodService, RunnerService } from "@pi-dash/services";
import type { IPod, IRunner, TPartialProject, TRunnerStatus } from "@pi-dash/types";
import type { TBadgeVariant } from "@pi-dash/ui";
import { AlertModalCore, Badge, Button, CustomSelect, Input, Tooltip } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { useWorkspace } from "@/hooks/store/use-workspace";
import { ProjectService } from "@/services/project";

const service = new RunnerService();
const podService = new PodService();
const projectService = new ProjectService();

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
  const workspaceSlug = currentWorkspace?.slug;
  const pageTitle = currentWorkspace?.name
    ? t("runners.page_title", { workspace: currentWorkspace.name })
    : t("runners.title");

  const { data: runners, mutate } = useSWR<IRunner[]>(
    workspaceId ? ["runners", workspaceId] : null,
    () => service.list(workspaceId),
    { refreshInterval: 5_000 }
  );

  const { data: pods, error: podsError } = useSWR<IPod[]>(
    workspaceId ? ["pods", workspaceId] : null,
    () => podService.list(workspaceId!),
    { refreshInterval: 30_000 }
  );

  // Projects feed the picker that decides which project a fresh runner
  // is bound to. The CLI's `--project` arg and the cloud's
  // `/api/v1/runner/register/` endpoint both require this — runners are
  // bound to one project for their lifetime.
  const { data: projects } = useSWR<TPartialProject[]>(workspaceSlug ? ["projects-lite", workspaceSlug] : null, () =>
    projectService.getProjectsLite(workspaceSlug!)
  );

  const [mintedToken, setMintedToken] = useState<string | null>(null);
  // Project identifier captured at mint time. We don't reuse the live
  // selector value because the user can change it while the
  // command-pre block is still on-screen, which would silently change
  // the displayed snippet under them.
  const [mintedProject, setMintedProject] = useState<string>("");
  const [label, setLabel] = useState("");
  const [selectedProject, setSelectedProject] = useState<string>("");
  const [minting, setMinting] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<IRunner | null>(null);
  const [revoking, setRevoking] = useState(false);
  const [origin, setOrigin] = useState("");
  const [justCopied, setJustCopied] = useState<"command" | "token" | null>(null);

  useEffect(() => {
    if (typeof window !== "undefined") setOrigin(window.location.origin);
  }, []);

  const apiOrigin = API_BASE_URL || origin;
  const configureCommand =
    mintedToken && mintedProject
      ? `pidash configure --url ${apiOrigin} --token ${mintedToken} --project ${mintedProject}`
      : "";
  const selectedProjectName = projects?.find((p) => p.identifier === selectedProject)?.name;

  async function copyToClipboard(text: string, kind: "command" | "token") {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setJustCopied(kind);
      window.setTimeout(() => setJustCopied((curr) => (curr === kind ? null : curr)), 2000);
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: t("runners.list.copy_failed"),
      });
    }
  }

  const activeCount = (runners ?? []).filter((r) => r.status !== "revoked").length;
  const atCap = activeCount >= MAX_RUNNERS_PER_USER;

  async function mint() {
    if (!workspaceId) return;
    if (!selectedProject) {
      setToast({
        type: TOAST_TYPE.WARNING,
        title: t("runners.toast.error_title"),
        message: t("runners.list.project_required"),
      });
      return;
    }
    setMinting(true);
    try {
      const result = await service.mintToken(workspaceId, label || undefined);
      setMintedToken(result.token);
      // Pin the project against the freshly-minted token so the
      // displayed snippet matches what the user picked, even if they
      // change the dropdown afterwards.
      setMintedProject(selectedProject);
      setLabel("");
      setSelectedProject("");
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
          <CustomSelect
            value={selectedProject}
            label={selectedProjectName ?? t("runners.list.project_placeholder")}
            onChange={(value: string) => setSelectedProject(value)}
            buttonClassName="border border-subtle min-w-[180px]"
            input
            maxHeight="lg"
            placement="bottom-end"
            disabled={!projects || projects.length === 0}
          >
            <>
              {(projects ?? []).map((p) => (
                <CustomSelect.Option key={p.id} value={p.identifier}>
                  {p.name}
                </CustomSelect.Option>
              ))}
            </>
          </CustomSelect>
          <Button onClick={mint} disabled={!workspaceId || atCap} loading={minting}>
            {atCap ? t("runners.list.cap_reached") : t("runners.list.mint")}
          </Button>
        </div>
        {mintedToken && (
          <div className="border-amber-500/40 bg-amber-500/10 mt-3 rounded border p-3 text-13 text-primary">
            <div className="font-medium">{t("runners.list.token_warning")}</div>

            <div className="mt-2 text-secondary">{t("runners.list.token_run_instructions")}</div>
            <pre className="font-mono mt-1 rounded border border-subtle bg-layer-1 p-2 text-11 whitespace-pre-wrap text-primary select-all">
              {configureCommand}
            </pre>
            <div className="mt-2">
              <Button size="sm" onClick={() => copyToClipboard(configureCommand, "command")}>
                {justCopied === "command" ? t("runners.list.copied") : t("runners.list.copy_command")}
              </Button>
            </div>

            <div className="mt-3 text-secondary">{t("runners.list.or_manual_token")}</div>
            <pre className="font-mono mt-1 rounded border border-subtle bg-layer-1 p-2 text-11 break-all text-primary select-all">
              {mintedToken}
            </pre>
            <div className="mt-2">
              <Button size="sm" variant="outline-primary" onClick={() => copyToClipboard(mintedToken, "token")}>
                {justCopied === "token" ? t("runners.list.copied") : t("runners.list.copy_token")}
              </Button>
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
                  <td colSpan={7} className="px-3 py-8 text-center text-secondary">
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
