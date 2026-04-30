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
import type {
  IConnection,
  IConnectionWithToken,
  IPod,
  IRunner,
  TConnectionStatus,
  TPartialProject,
  TRunnerStatus,
} from "@pi-dash/types";
import type { TBadgeVariant } from "@pi-dash/ui";
import { AlertModalCore, Badge, Button, CustomSelect, Input, Tooltip } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { useWorkspace } from "@/hooks/store/use-workspace";
import { ProjectService } from "@/services/project";

const service = new RunnerService();
const podService = new PodService();
const projectService = new ProjectService();

const STATUS_BADGE_VARIANT: Record<TRunnerStatus, TBadgeVariant> = {
  online: "accent-success",
  busy: "accent-primary",
  offline: "accent-neutral",
};

const CONNECTION_STATUS_BADGE: Record<TConnectionStatus, TBadgeVariant> = {
  pending: "accent-neutral",
  active: "accent-success",
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

  const { data: connections, mutate: mutateConnections } = useSWR<IConnection[]>(
    workspaceId ? ["connections", workspaceId] : null,
    () => service.listConnections(),
    { refreshInterval: 10_000 }
  );

  const { data: pods, error: podsError } = useSWR<IPod[]>(
    workspaceId ? ["pods", workspaceId] : null,
    () => podService.list(workspaceId!),
    { refreshInterval: 30_000 }
  );

  // Project list feeds the per-connection "next: pidash runner add" snippet
  // so the user can copy a complete command without typing the slug.
  const { data: projects } = useSWR<TPartialProject[]>(workspaceSlug ? ["projects-lite", workspaceSlug] : null, () =>
    projectService.getProjectsLite(workspaceSlug!)
  );

  const [pendingConnection, setPendingConnection] = useState<IConnectionWithToken | null>(null);
  const [connectionName, setConnectionName] = useState("");
  const [creating, setCreating] = useState(false);
  const [snippetProject, setSnippetProject] = useState<string>("");
  const [deleteRunner, setDeleteRunner] = useState<IRunner | null>(null);
  const [deleteConnection, setDeleteConnection] = useState<IConnection | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [origin, setOrigin] = useState("");
  const [justCopied, setJustCopied] = useState<string | null>(null);

  useEffect(() => {
    if (typeof window !== "undefined") setOrigin(window.location.origin);
  }, []);

  const apiOrigin = API_BASE_URL || origin;
  const connectCommand = pendingConnection
    ? `pidash connect --url ${apiOrigin} --token ${pendingConnection.enrollment_token}`
    : "";
  const projectArg = snippetProject || "<PROJECT>";
  const runnerAddCommand = [
    "pidash runner add \\",
    `  --project ${projectArg} \\`,
    "  --name <NAME> \\",
    "  --pod <POD> \\",
    "  --working-dir <WORKING_DIR> \\",
    "  --agent <codex|claude-code>",
  ].join("\n");

  async function copyToClipboard(text: string, key: string) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setJustCopied(key);
      window.setTimeout(() => setJustCopied((curr) => (curr === key ? null : curr)), 2000);
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: t("runners.list.copy_failed"),
      });
    }
  }

  async function createConnection() {
    if (!workspaceId) return;
    setCreating(true);
    try {
      const result = await service.createConnection(workspaceId, connectionName || undefined);
      setPendingConnection(result);
      setConnectionName("");
      mutateConnections();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("runners.connections.create_failed"),
      });
    } finally {
      setCreating(false);
    }
  }

  async function dismissPending() {
    if (!pendingConnection) return;
    // If the daemon hasn't enrolled yet, dismissing means "I changed my
    // mind" — delete the row so a stale pending connection doesn't sit
    // around forever. Pull a fresh list first so a just-enrolled
    // connection (status flipped to active between mint and dismiss
    // within the SWR refresh window) isn't accidentally deleted.
    try {
      const fresh = await service.listConnections();
      const latest = fresh.find((c) => c.id === pendingConnection.id);
      if (latest?.status === "pending") {
        await service.deleteConnection(pendingConnection.id);
      }
    } catch {
      // Best-effort cleanup; don't block the user from hiding the panel.
    }
    setPendingConnection(null);
    mutateConnections();
  }

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
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("runners.list.delete_failed"),
      });
    } finally {
      setDeleting(false);
    }
  }

  async function confirmDeleteConnection() {
    if (!deleteConnection) return;
    setDeleting(true);
    try {
      await service.deleteConnection(deleteConnection.id);
      setDeleteConnection(null);
      mutateConnections();
      mutateRunners();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("runners.connections.delete_failed"),
      });
    } finally {
      setDeleting(false);
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <PageHead title={pageTitle} />

      {/* ───────── Connections ───────── */}
      <section className="rounded-md border border-subtle p-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-1.5">
              <div className="text-13 font-medium text-primary">{t("runners.connections.title")}</div>
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
            <div className="text-13 text-secondary">{t("runners.connections.help")}</div>
          </div>
        </div>

        <div className="mt-3 flex items-center gap-2">
          <Input
            value={connectionName}
            onChange={(e) => setConnectionName(e.target.value)}
            placeholder={t("runners.connections.name_placeholder")}
            className="flex-1"
          />
          <Button onClick={createConnection} disabled={!workspaceId} loading={creating}>
            {t("runners.connections.add")}
          </Button>
        </div>

        {pendingConnection && (
          <div className="border-amber-500/40 bg-amber-500/10 mt-3 rounded border p-3 text-13 text-primary">
            <div className="font-medium">{t("runners.connections.token_warning")}</div>

            <div className="mt-2 text-secondary">{t("runners.connections.token_run_instructions")}</div>
            <pre className="font-mono mt-1 rounded border border-subtle bg-layer-1 p-2 text-11 whitespace-pre-wrap text-primary select-all">
              {connectCommand}
            </pre>
            <div className="mt-2">
              <Button size="sm" onClick={() => copyToClipboard(connectCommand, "connect")}>
                {justCopied === "connect" ? t("runners.list.copied") : t("runners.connections.copy_command")}
              </Button>
            </div>

            <div className="mt-3 text-secondary">{t("runners.connections.next_step_runner")}</div>
            <div className="mt-1 flex items-center gap-2">
              <CustomSelect
                value={snippetProject}
                label={
                  projects?.find((p) => p.identifier === snippetProject)?.name ?? t("runners.list.project_placeholder")
                }
                onChange={(value: string) => setSnippetProject(value)}
                buttonClassName="border border-subtle min-w-[180px]"
                input
                maxHeight="lg"
                placement="bottom-start"
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
              <Button size="sm" variant="outline-primary" onClick={() => copyToClipboard(runnerAddCommand, "runner")}>
                {justCopied === "runner" ? t("runners.list.copied") : t("runners.connections.copy_runner_command")}
              </Button>
            </div>
            <pre className="font-mono mt-2 rounded border border-subtle bg-layer-1 p-2 text-11 whitespace-pre-wrap text-primary select-all">
              {runnerAddCommand}
            </pre>

            <div className="mt-3">
              <Button variant="outline-primary" size="sm" onClick={dismissPending}>
                {t("runners.connections.dismiss_token")}
              </Button>
            </div>
          </div>
        )}

        <div className="mt-3 overflow-x-auto rounded-md border border-subtle">
          <table className="w-full text-13">
            <thead className="bg-layer-1 text-left text-secondary">
              <tr>
                <th className="px-3 py-2">{t("runners.connections.columns.name")}</th>
                <th className="px-3 py-2">{t("runners.connections.columns.host")}</th>
                <th className="px-3 py-2">{t("runners.connections.columns.status")}</th>
                <th className="px-3 py-2">{t("runners.connections.columns.runner_count")}</th>
                <th className="px-3 py-2">{t("runners.connections.columns.last_seen")}</th>
                <th className="px-3 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {(connections ?? []).map((c) => (
                <tr key={c.id} className="border-t border-subtle">
                  <td className="font-mono px-3 py-2 text-12">{c.name}</td>
                  <td className="px-3 py-2">{c.host_label || "—"}</td>
                  <td className="px-3 py-2">
                    <Badge variant={CONNECTION_STATUS_BADGE[c.status]} size="sm">
                      {t(`runners.connections.status.${c.status}`)}
                    </Badge>
                  </td>
                  <td className="px-3 py-2">{c.runner_count}</td>
                  <td className="px-3 py-2">{c.last_seen_at ? new Date(c.last_seen_at).toLocaleString() : "—"}</td>
                  <td className="px-3 py-2 text-right">
                    <Button variant="tertiary-danger" size="sm" onClick={() => setDeleteConnection(c)}>
                      {t("runners.connections.delete")}
                    </Button>
                  </td>
                </tr>
              ))}
              {(connections ?? []).length === 0 && (
                <tr>
                  <td colSpan={6} className="px-3 py-8 text-center text-secondary">
                    {t("runners.connections.empty")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>

      {/* ───────── Pods (read-only summary) ───────── */}
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

      {/* ───────── Runners ───────── */}
      <section>
        <div className="mb-2 text-13 font-medium text-primary">{t("runners.list.connected_runners")}</div>
        <div className="overflow-x-auto rounded-md border border-subtle">
          <table className="w-full text-13">
            <thead className="bg-layer-1 text-left text-secondary">
              <tr>
                <th className="px-3 py-2">{t("runners.list.columns.name")}</th>
                <th className="px-3 py-2">{t("runners.list.columns_connection")}</th>
                <th className="px-3 py-2">{t("runners.list.columns_pod")}</th>
                <th className="px-3 py-2">{t("runners.list.columns.status")}</th>
                <th className="px-3 py-2">{t("runners.list.columns.os_arch")}</th>
                <th className="px-3 py-2">{t("runners.list.columns.version")}</th>
                <th className="px-3 py-2">{t("runners.list.columns.last_heartbeat")}</th>
                <th className="px-3 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {(runners ?? []).map((r) => {
                const conn = (connections ?? []).find((c) => c.id === r.connection);
                return (
                  <tr key={r.id} className="border-t border-subtle">
                    <td className="font-mono px-3 py-2 text-11">{r.name}</td>
                    <td className="font-mono px-3 py-2 text-11">{conn?.name ?? "—"}</td>
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
                      <Button variant="tertiary-danger" size="sm" onClick={() => setDeleteRunner(r)}>
                        {t("runners.list.delete")}
                      </Button>
                    </td>
                  </tr>
                );
              })}
              {(runners ?? []).length === 0 && (
                <tr>
                  <td colSpan={8} className="px-3 py-8 text-center text-secondary">
                    {t("runners.list.empty")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>

      <AlertModalCore
        isOpen={!!deleteRunner}
        handleClose={() => (deleting ? null : setDeleteRunner(null))}
        handleSubmit={confirmDeleteRunner}
        isSubmitting={deleting}
        title={t("runners.list.delete_confirm_title")}
        content={t("runners.list.delete_confirm_body")}
        primaryButtonText={{ default: t("runners.list.delete"), loading: t("runners.list.delete") }}
      />
      <AlertModalCore
        isOpen={!!deleteConnection}
        handleClose={() => (deleting ? null : setDeleteConnection(null))}
        handleSubmit={confirmDeleteConnection}
        isSubmitting={deleting}
        title={t("runners.connections.delete_confirm_title")}
        content={t("runners.connections.delete_confirm_body")}
        primaryButtonText={{
          default: t("runners.connections.delete"),
          loading: t("runners.connections.delete"),
        }}
      />
    </div>
  );
});

export default RunnersListPage;
