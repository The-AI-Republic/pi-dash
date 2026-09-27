/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo, useState } from "react";
import { observer } from "mobx-react";
import { Link, useNavigate } from "react-router";
import useSWR from "swr";
// pi dash imports
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import type { IAgentRun, IAgentRunPage } from "@pi-dash/types";
import { Badge, Button, ToggleSwitch } from "@pi-dash/ui";
// components
import { EditSchedulerBindingModal } from "@/components/project/scheduler-bindings/edit-binding-modal";
import { UninstallSchedulerBindingModal } from "@/components/project/scheduler-bindings/uninstall-binding-modal";
import { RunStatusBadge } from "@/components/runners/run-status-badge";
// hooks
import { useMember } from "@/hooks/store/use-member";
import { useUserPermissions } from "@/hooks/store/user";
import { DEFAULT_SCHEDULER_COLOR } from "./constants";
import { humanizeRrule } from "./rrule-text";

const schedulerService = new SchedulerService();

type Props = {
  workspaceSlug: string;
  projectId: string;
  bindingId: string;
};

/** "3m 12s" for a finished run, null when it never started or hasn't ended. */
function formatDuration(startedAt: string | null, endedAt: string | null): string | null {
  if (!startedAt || !endedAt) return null;
  const ms = new Date(endedAt).getTime() - new Date(startedAt).getTime();
  if (!Number.isFinite(ms) || ms < 0) return null;
  const totalSeconds = Math.round(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}

/** One-line result cell: the failure excerpt for failed runs, else the done summary. */
function runResultExcerpt(run: IAgentRun): string {
  if (run.error) return run.error;
  const summary = run.done_payload?.summary;
  return typeof summary === "string" ? summary : "";
}

/**
 * Scheduler-binding detail page body: full install config, the resolved
 * prompt a run actually receives, and the paginated AgentRun history this
 * binding has fired. Rows link to the existing run detail view; mutations
 * (edit / uninstall / toggle) reuse the list page's modals and are gated on
 * PROJECT ADMIN like everywhere else on the scheduler surface.
 */
export const SchedulerBindingDetail = observer(function SchedulerBindingDetail(props: Props) {
  const { workspaceSlug, projectId, bindingId } = props;
  const { allowPermissions } = useUserPermissions();
  const { getUserDetails } = useMember();
  const { t } = useTranslation();
  const navigate = useNavigate();

  const canManage = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.PROJECT, workspaceSlug, projectId);
  const isWorkspaceAdmin = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE, workspaceSlug);

  const {
    data: binding,
    error: bindingError,
    mutate: mutateBinding,
  } = useSWR<ISchedulerBinding>(
    workspaceSlug && projectId && bindingId ? ["scheduler-binding-detail", workspaceSlug, projectId, bindingId] : null,
    () => schedulerService.retrieveBinding(workspaceSlug, projectId, bindingId)
  );

  const [page, setPage] = useState(1);
  const { data: runsPage } = useSWR<IAgentRunPage>(
    workspaceSlug && projectId && bindingId
      ? ["scheduler-binding-runs", workspaceSlug, projectId, bindingId, page]
      : null,
    () => schedulerService.listBindingRuns(workspaceSlug, projectId, bindingId, page),
    { refreshInterval: 15_000 }
  );

  const [editOpen, setEditOpen] = useState(false);
  const [uninstallOpen, setUninstallOpen] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [toggling, setToggling] = useState(false);

  const scheduleText = useMemo(() => {
    if (!binding) return null;
    return humanizeRrule(binding.rrule, binding.dtstart) ?? t("Once at dtstart");
  }, [binding, t]);

  const listPath = `/${workspaceSlug}/projects/${projectId}/schedulers/list`;
  const formatTs = (ts: string | null) => (ts ? new Date(ts).toLocaleString() : t("(never)"));

  async function handleToggle(next: boolean) {
    if (!binding || toggling) return;
    setToggling(true);
    try {
      const updated = await schedulerService.updateBinding(workspaceSlug, projectId, binding.id, { enabled: next });
      await mutateBinding(updated, { revalidate: false });
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: err?.error ?? t("Could not update the install."),
      });
    } finally {
      setToggling(false);
    }
  }

  if (bindingError) {
    return (
      <div className="flex flex-col gap-3 p-6">
        <p className="text-13 text-secondary">
          {t("This scheduler install is not available. It may have been uninstalled.")}
        </p>
        <div>
          <Link to={listPath} className="text-13 font-medium text-primary hover:underline">
            ← {t("Back to schedulers")}
          </Link>
        </div>
      </div>
    );
  }

  if (!binding) {
    return <div className="p-6 text-13 text-secondary">{t("Loading…")}</div>;
  }

  const actorName = binding.actor ? (getUserDetails(binding.actor)?.display_name ?? binding.actor) : null;
  const runs = runsPage?.results ?? [];
  const totalPages = runsPage?.total_pages ?? 1;
  const totalCount = runsPage?.total_count ?? 0;

  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <Link to={listPath} className="text-13 text-secondary hover:text-primary hover:underline">
          ← {t("Back to schedulers")}
        </Link>
      </div>

      <header className="flex items-start justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <span
              className="inline-block h-4 w-4 flex-shrink-0 rounded-sm"
              style={{ backgroundColor: binding.scheduler_color || DEFAULT_SCHEDULER_COLOR }}
              aria-hidden="true"
            />
            <h1 className="text-16 font-semibold text-primary">{binding.scheduler_name}</h1>
            {binding.scheduler_source && (
              <Badge variant="accent-neutral" size="sm">
                {binding.scheduler_source}
              </Badge>
            )}
            {binding.scheduler_is_enabled === false && (
              <Badge variant="accent-warning" size="sm">
                {t("Disabled for the whole workspace")}
              </Badge>
            )}
          </div>
          <div className="mt-1 text-12 text-secondary">
            <code>{binding.scheduler_slug}</code>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {isWorkspaceAdmin && (
            <Link to={`/${workspaceSlug}/schedulers`} className="text-13 text-secondary hover:text-primary">
              {t("View workspace definition")}
            </Link>
          )}
          {canManage && (
            <>
              <Button variant="neutral-primary" size="sm" onClick={() => setEditOpen(true)}>
                {t("Edit")}
              </Button>
              <Button variant="tertiary-danger" size="sm" onClick={() => setUninstallOpen(true)}>
                {t("Uninstall")}
              </Button>
            </>
          )}
        </div>
      </header>

      {binding.last_error && (
        <div className="rounded-md border border-danger-subtle bg-danger-subtle/20 p-3 text-13">
          <div className="font-medium text-danger-primary">{t("Last error")}</div>
          <pre className="mt-1 text-11 whitespace-pre-wrap text-primary">{binding.last_error}</pre>
        </div>
      )}

      <section className="rounded-md border border-subtle p-4">
        <h2 className="text-14 font-medium text-primary">{t("Configuration")}</h2>
        <dl className="mt-3 grid grid-cols-1 gap-x-8 gap-y-3 text-13 md:grid-cols-2">
          <ConfigRow label={t("Schedule")} value={scheduleText ?? ""} title={binding.rrule || undefined} />
          <ConfigRow label={t("Starts at")} value={formatTs(binding.dtstart)} />
          <ConfigRow label={t("Time zone")} value={binding.tzid || "UTC"} />
          <ConfigRow label={t("Next run")} value={formatTs(binding.next_run_at)} />
          <ConfigRow label={t("Outcome mode")} value={binding.outcome_mode} />
          <ConfigRow label={t("Pod")} value={binding.pod_name ?? t("(default pod)")} />
          {binding.rdates.length > 0 && (
            <ConfigRow
              label={t("Extra dates")}
              value={binding.rdates.map((d) => new Date(d).toLocaleString()).join(", ")}
            />
          )}
          {binding.exdates.length > 0 && (
            <ConfigRow
              label={t("Skipped dates")}
              value={binding.exdates.map((d) => new Date(d).toLocaleString()).join(", ")}
            />
          )}
          {actorName && <ConfigRow label={t("Installed by")} value={actorName} />}
          <div>
            <dt className="text-12 tracking-wide text-tertiary uppercase">{t("Enabled")}</dt>
            <dd className="mt-1 flex items-center gap-2">
              <ToggleSwitch
                value={binding.enabled}
                onChange={handleToggle}
                disabled={toggling || !canManage}
                aria-label={binding.enabled ? t("Disable scheduler") : t("Enable scheduler")}
              />
              <span className="text-12 text-secondary">{binding.enabled ? t("Enabled") : t("Disabled")}</span>
            </dd>
          </div>
        </dl>
        {binding.extra_context && (
          <div className="mt-4">
            <div className="text-12 tracking-wide text-tertiary uppercase">{t("Project context")}</div>
            <pre className="mt-1 rounded bg-layer-1 p-2 text-11 whitespace-pre-wrap">{binding.extra_context}</pre>
          </div>
        )}
        {typeof binding.resolved_prompt === "string" && (
          <div className="mt-4">
            <button
              type="button"
              className="text-13 font-medium text-primary hover:underline"
              onClick={() => setPromptOpen((v) => !v)}
            >
              {promptOpen ? t("Hide resolved prompt") : t("Show resolved prompt")}
            </button>
            {promptOpen && (
              <pre className="mt-2 rounded bg-layer-1 p-2 text-11 whitespace-pre-wrap">{binding.resolved_prompt}</pre>
            )}
          </div>
        )}
      </section>

      <section className="rounded-md border border-subtle">
        <div className="flex items-center justify-between border-b border-subtle px-4 py-3">
          <h2 className="text-14 font-medium text-primary">{t("Run history")}</h2>
          <span className="text-12 text-secondary">
            {t("{count} runs", { count: binding.run_count ?? totalCount })}
          </span>
        </div>
        <table className="w-full text-13">
          <thead className="bg-layer-1 text-left text-secondary">
            <tr>
              <th className="px-3 py-2">{t("Started")}</th>
              <th className="px-3 py-2">{t("Ended")}</th>
              <th className="px-3 py-2">{t("Status")}</th>
              <th className="px-3 py-2">{t("Duration")}</th>
              <th className="px-3 py-2">{t("Pod")}</th>
              <th className="px-3 py-2">{t("Result")}</th>
            </tr>
          </thead>
          <tbody>
            {runs.map((run) => (
              <tr
                key={run.id}
                onClick={() => navigate(`/${workspaceSlug}/projects/${projectId}/runners/runs/${run.id}`)}
                className="cursor-pointer border-t border-subtle hover:bg-layer-1"
              >
                <td className="px-3 py-2 whitespace-nowrap">
                  {run.started_at
                    ? new Date(run.started_at).toLocaleString()
                    : t("Queued {ts}", { ts: new Date(run.created_at).toLocaleString() })}
                </td>
                <td className="px-3 py-2 whitespace-nowrap">{formatTs(run.ended_at)}</td>
                <td className="px-3 py-2">
                  <RunStatusBadge status={run.status} t={t} />
                </td>
                <td className="px-3 py-2 whitespace-nowrap">{formatDuration(run.started_at, run.ended_at) ?? "—"}</td>
                <td className="px-3 py-2 whitespace-nowrap">{run.pod_detail?.name ?? "—"}</td>
                <td
                  className={`max-w-[280px] truncate px-3 py-2 text-11 ${run.error ? "text-danger-primary" : "text-secondary"}`}
                  title={runResultExcerpt(run) || undefined}
                >
                  {runResultExcerpt(run)}
                </td>
              </tr>
            ))}
            {runsPage && runs.length === 0 && (
              <tr>
                <td colSpan={6} className="px-3 py-8 text-center text-secondary">
                  {binding.enabled
                    ? binding.next_run_at
                      ? t("No runs yet — next run at {ts}", { ts: new Date(binding.next_run_at).toLocaleString() })
                      : t("No runs yet.")
                    : t("Scheduler is disabled — it will not fire until re-enabled.")}
                </td>
              </tr>
            )}
            {!runsPage && (
              <tr>
                <td colSpan={6} className="px-3 py-8 text-center text-secondary">
                  {t("Loading…")}
                </td>
              </tr>
            )}
          </tbody>
        </table>
        {totalCount > 0 && (
          <div className="flex items-center justify-between gap-2 border-t border-subtle bg-layer-1 px-3 py-2 text-11 text-secondary">
            <span>{t("Page {page} of {total}", { page, total: totalPages })}</span>
            <div className="flex items-center gap-2">
              <Button variant="neutral-primary" size="sm" disabled={page <= 1} onClick={() => setPage(page - 1)}>
                {t("Previous")}
              </Button>
              <Button
                variant="neutral-primary"
                size="sm"
                disabled={page >= totalPages}
                onClick={() => setPage(page + 1)}
              >
                {t("Next")}
              </Button>
            </div>
          </div>
        )}
      </section>

      <EditSchedulerBindingModal
        isOpen={editOpen}
        onClose={() => setEditOpen(false)}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        binding={binding}
        onUpdated={() => mutateBinding()}
      />
      <UninstallSchedulerBindingModal
        isOpen={uninstallOpen}
        onClose={() => setUninstallOpen(false)}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        binding={uninstallOpen ? binding : null}
        onUninstalled={() => navigate(listPath)}
      />
    </div>
  );
});

function ConfigRow({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <div>
      <dt className="text-12 tracking-wide text-tertiary uppercase">{label}</dt>
      <dd className="mt-1 text-13 text-primary" title={title}>
        {value}
      </dd>
    </div>
  );
}
