// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One install's detail page (parity/specs/project-settings-part-f.md
// §PSET-059; parity/specs/agents.md §3 AGT-013). Project admins manage the
// install here — enable switch, edit dialog, uninstall confirmation — while
// every project member reads the configuration, the composed prompt and the
// run history. The history re-reads on a fifteen-second poll and pages
// through the binding's own runs endpoint. A definition or install the API
// no longer answers with lands on a not-available notice with a way back.

import { Button, Skeleton, Switch } from "@pidash/kit";
import { Link, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { ROLE_ADMIN, useWorkspace } from "../../core/session/index.js";
import { useDocumentHead } from "../../shared/shell/index.js";
import { useWorkspaceDirectory } from "../project-settings/index.js";
import { useWorkspaceMembership } from "../auth/index.js";
import { useProjectRoles } from "../auth/index.js";
import { isProjectAdmin } from "../projects/index.js";
import {
  configRows,
  emptyHistoryText,
  formatDuration,
  formatMoment,
  installedByName,
  isSeriesCompleted,
  pagerTotalPages,
  runResultExcerpt,
  runStartedLabel,
  runStatusLabel,
  runStatusTone,
  SERIES_COMPLETED_BADGE,
  SERIES_COMPLETED_TEXT,
  sourceLabel,
} from "./bindingDetail.js";
import { EditInstallDialog } from "./EditInstallDialog.js";
import { useSchedulerBindingDetail, useSchedulerBindingRuns, useToggleSchedulerBinding } from "./installData.js";
import type { RunTone } from "./bindingDetail.js";
import { UninstallInstallDialog } from "./UninstallInstallDialog.js";
import type { SchedulerBinding } from "@pidash/api-client";

export const NOT_AVAILABLE_TEXT = "This scheduler install is not available. It may have been uninstalled.";
export const BACK_TO_LIST_LABEL = "Back to schedulers";
export const WORKSPACE_DEFINITION_LABEL = "View workspace definition";
export const PROMPT_SHOW_LABEL = "Show resolved prompt";
export const PROMPT_HIDE_LABEL = "Hide resolved prompt";

export interface BindingDetailPageProps {
  workspaceSlug: string;
  projectId: string;
  bindingId: string;
}

function BackLink({ workspaceSlug, projectId }: { workspaceSlug: string; projectId: string }): React.ReactElement {
  return (
    <Link
      to="/$ws/projects/$projectId/schedulers/list"
      params={{ ws: workspaceSlug, projectId }}
      className="text-meta text-(--text-muted) hover:text-(--text)"
    >
      ← {BACK_TO_LIST_LABEL}
    </Link>
  );
}

/** A definition or install the API no longer serves. */
export function BindingNotAvailable({
  workspaceSlug,
  projectId,
}: {
  workspaceSlug: string;
  projectId: string;
}): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-4) p-(--space-6)">
      <p className="text-body text-(--text-muted)">{NOT_AVAILABLE_TEXT}</p>
      <div>
        <BackLink workspaceSlug={workspaceSlug} projectId={projectId} />
      </div>
    </div>
  );
}

function DetailSkeleton(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading scheduler install" className="flex flex-col gap-(--space-5) p-(--space-6)">
      <Skeleton width="220px" height="24px" />
      <Skeleton width="60%" height="16px" />
      <Skeleton height="120px" />
      <Skeleton height="180px" />
    </div>
  );
}

function StatusMark({ status }: { status: string }): React.ReactElement {
  const tone = runStatusTone(status);
  const toneClass: Record<RunTone, string> = {
    success: "border-(--success) text-(--success)",
    danger: "border-(--danger) text-(--danger)",
    accent: "border-(--accent) text-(--accent)",
    neutral: "border-(--border) text-(--text-muted)",
  };
  return (
    <span
      className={`text-meta inline-flex items-center rounded-(--radius-full) border px-(--space-2) py-0.5 ${toneClass[tone]}`}
    >
      {runStatusLabel(status)}
    </span>
  );
}

export interface BindingDetailBodyProps {
  workspaceSlug: string;
  projectId: string;
  bindingId: string;
  canManage: boolean;
  canSeeWorkspaceDefinition: boolean;
}

export function BindingDetailBody({
  workspaceSlug,
  projectId,
  bindingId,
  canManage,
  canSeeWorkspaceDefinition,
}: BindingDetailBodyProps): React.ReactElement {
  const navigate = useNavigate();
  const detail = useSchedulerBindingDetail(workspaceSlug, projectId, bindingId);
  const [page, setPage] = React.useState(1);
  const runs = useSchedulerBindingRuns(workspaceSlug, projectId, bindingId, page);
  const toggle = useToggleSchedulerBinding(workspaceSlug);
  const directory = useWorkspaceDirectory(workspaceSlug);
  const [revealed, setRevealed] = React.useState(false);
  const [editing, setEditing] = React.useState<SchedulerBinding | null>(null);
  const [uninstalling, setUninstalling] = React.useState<SchedulerBinding | null>(null);
  const [flipping, setFlipping] = React.useState(false);

  if (detail.isPending) return <DetailSkeleton />;
  if (detail.isError) return <BindingNotAvailable workspaceSlug={workspaceSlug} projectId={projectId} />;
  const binding = detail.data;
  const rows = configRows(binding, installedByName(binding.actor, directory.data ?? []));
  const completed = isSeriesCompleted(binding);
  const runPage = runs.data;
  const totalPages = pagerTotalPages(runPage);

  // The switch disables itself while its own flip is in flight, so this
  // only runs for the first press of a pair.
  const flip = (enabled: boolean): void => {
    setFlipping(true);
    toggle.mutate({ projectId, bindingId, enabled }, { onSettled: () => setFlipping(false) });
  };

  return (
    <div className="flex flex-col gap-(--space-6) p-(--space-6)">
      <div>
        <BackLink workspaceSlug={workspaceSlug} projectId={projectId} />
      </div>

      <header className="flex flex-wrap items-start justify-between gap-(--space-4)">
        <div className="flex flex-col gap-(--space-1)">
          <div className="flex flex-wrap items-center gap-(--space-2)">
            <span
              aria-hidden="true"
              className="inline-block size-3.5 shrink-0 rounded-sm"
              style={{ backgroundColor: binding.scheduler_color }}
            />
            <h1 className="text-h3 text-(--text)">{binding.scheduler_name}</h1>
            <span className="text-meta rounded-(--radius-full) border border-(--border) px-(--space-2) py-0.5 text-(--text-muted)">
              {sourceLabel(binding.scheduler_source)}
            </span>
            {binding.scheduler_is_enabled ? null : (
              <span className="text-meta rounded-(--radius-full) border border-(--warning) px-(--space-2) py-0.5 text-(--warning)">
                Disabled for the whole workspace
              </span>
            )}
            {completed ? (
              <span
                className="text-meta rounded-(--radius-full) border border-(--success) px-(--space-2) py-0.5 text-(--success)"
                title={SERIES_COMPLETED_TEXT}
              >
                {SERIES_COMPLETED_BADGE}
              </span>
            ) : null}
          </div>
          <code className="text-meta text-(--text-muted)">{binding.scheduler_slug}</code>
        </div>
        <div className="flex items-center gap-(--space-2)">
          {canSeeWorkspaceDefinition ? (
            <Link
              to="/$ws/schedulers"
              params={{ ws: workspaceSlug }}
              className="text-meta text-(--text-muted) hover:text-(--text)"
            >
              {WORKSPACE_DEFINITION_LABEL}
            </Link>
          ) : null}
          {canManage ? (
            <>
              <Button size="small" onClick={() => setEditing(binding)}>
                Edit
              </Button>
              <Button size="small" variant="danger" onClick={() => setUninstalling(binding)}>
                Uninstall
              </Button>
            </>
          ) : null}
        </div>
      </header>

      {binding.last_error.trim() === "" ? null : (
        <div className="rounded-(--radius-control) border border-(--danger) bg-(--surface) p-(--space-3)">
          <div className="text-body font-medium text-(--danger)">Last error</div>
          <pre className="text-meta mt-(--space-1) whitespace-pre-wrap text-(--text)">{binding.last_error}</pre>
        </div>
      )}

      <section aria-label="Configuration" className="rounded-(--radius-control) border border-(--border) p-(--space-4)">
        <h2 className="text-body font-medium text-(--text)">Configuration</h2>
        <dl className="text-body mt-(--space-3) grid grid-cols-1 gap-x-(--space-8) gap-y-(--space-3) md:grid-cols-2">
          {rows.map((row) => (
            <div key={row.label}>
              <dt className="text-meta text-(--text-muted) uppercase">{row.label}</dt>
              <dd className="mt-(--space-1) text-(--text)" {...(row.title === undefined ? {} : { title: row.title })}>
                {row.value}
              </dd>
            </div>
          ))}
          <div>
            <dt className="text-meta text-(--text-muted) uppercase">Enabled</dt>
            <dd className="mt-(--space-1) flex items-center gap-(--space-2)">
              <Switch
                label={binding.enabled ? "Disable scheduler" : "Enable scheduler"}
                checked={binding.enabled}
                disabled={!canManage || flipping}
                onCheckedChange={flip}
              />
              <span className="text-meta text-(--text-muted)">{binding.enabled ? "Enabled" : "Disabled"}</span>
            </dd>
          </div>
        </dl>

        {binding.extra_context.trim() === "" ? null : (
          <div className="mt-(--space-4)">
            <div className="text-meta text-(--text-muted) uppercase">Project context</div>
            <pre className="text-meta mt-(--space-1) rounded-(--radius-control) bg-(--subtle) p-(--space-2) whitespace-pre-wrap text-(--text)">
              {binding.extra_context}
            </pre>
          </div>
        )}

        {binding.resolved_prompt.trim() === "" ? null : (
          <div className="mt-(--space-4)">
            <button
              type="button"
              className="text-body font-medium text-(--text) hover:underline"
              onClick={() => setRevealed((current) => !current)}
            >
              {revealed ? PROMPT_HIDE_LABEL : PROMPT_SHOW_LABEL}
            </button>
            {revealed ? (
              <pre className="text-meta mt-(--space-2) rounded-(--radius-control) bg-(--subtle) p-(--space-2) whitespace-pre-wrap text-(--text)">
                {binding.resolved_prompt}
              </pre>
            ) : null}
          </div>
        )}
      </section>

      <section aria-label="Run history" className="rounded-(--radius-control) border border-(--border)">
        <div className="flex items-center justify-between border-b border-(--border) px-(--space-4) py-(--space-3)">
          <h2 className="text-body font-medium text-(--text)">Run history</h2>
          <span className="text-meta text-(--text-muted)">{`${binding.run_count} runs`}</span>
        </div>
        <div className="overflow-auto">
          <table className="text-body w-full text-left">
            <thead className="text-meta bg-(--subtle) text-(--text-muted)">
              <tr>
                {["Started", "Ended", "Status", "Duration", "Pod", "Result"].map((label) => (
                  <th key={label} scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                    {label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {runs.isPending ? (
                <tr>
                  <td colSpan={6} className="px-(--space-3) py-(--space-8) text-center text-(--text-muted)">
                    Loading…
                  </td>
                </tr>
              ) : null}
              {runs.isError ? (
                <tr>
                  <td colSpan={6} className="px-(--space-3) py-(--space-8)">
                    <div role="alert" className="flex flex-col items-center gap-(--space-3)">
                      <p className="text-body text-(--text-muted)">
                        The run history could not be read. This scheduler install may have been removed.
                      </p>
                      <BackLink workspaceSlug={workspaceSlug} projectId={projectId} />
                    </div>
                  </td>
                </tr>
              ) : null}
              {runPage !== undefined && runPage.results.length === 0 ? (
                <tr>
                  <td colSpan={6} className="px-(--space-3) py-(--space-8) text-center text-(--text-muted)">
                    {emptyHistoryText(binding)}
                  </td>
                </tr>
              ) : null}
              {runPage?.results.map((run) => {
                const excerpt = runResultExcerpt(run);
                return (
                  <tr
                    key={run.id}
                    className="cursor-pointer border-t border-(--border) hover:bg-(--subtle)"
                    onClick={() => {
                      void navigate({ href: `/${workspaceSlug}/projects/${projectId}/runners/runs/${run.id}` });
                    }}
                  >
                    <td className="px-(--space-3) py-(--space-2) whitespace-nowrap">{runStartedLabel(run)}</td>
                    <td className="px-(--space-3) py-(--space-2) whitespace-nowrap">{formatMoment(run.ended_at)}</td>
                    <td className="px-(--space-3) py-(--space-2)">
                      <StatusMark status={run.status} />
                    </td>
                    <td className="px-(--space-3) py-(--space-2) whitespace-nowrap">
                      {formatDuration(run.started_at, run.ended_at)}
                    </td>
                    <td className="px-(--space-3) py-(--space-2) whitespace-nowrap">{run.pod_detail.name}</td>
                    <td
                      className={`text-meta max-w-[280px] truncate px-(--space-3) py-(--space-2) ${
                        run.error === "" ? "text-(--text-muted)" : "text-(--danger)"
                      }`}
                      title={excerpt === "" ? undefined : excerpt}
                    >
                      {excerpt}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        {runPage === undefined || runPage.total_count === 0 ? null : (
          <div className="text-meta flex items-center justify-between gap-(--space-2) border-t border-(--border) bg-(--subtle) px-(--space-3) py-(--space-2) text-(--text-muted)">
            <span>{`Page ${page} of ${totalPages}`}</span>
            <div className="flex items-center gap-(--space-2)">
              <Button size="small" disabled={page <= 1} onClick={() => setPage((current) => Math.max(1, current - 1))}>
                Previous
              </Button>
              <Button
                size="small"
                disabled={page >= totalPages}
                onClick={() => setPage((current) => Math.min(totalPages, current + 1))}
              >
                Next
              </Button>
            </div>
          </div>
        )}
      </section>

      <EditInstallDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        binding={editing}
        onClose={() => setEditing(null)}
        onSaved={() => {
          void detail.refetch();
        }}
      />
      <UninstallInstallDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        binding={uninstalling}
        onClose={() => setUninstalling(null)}
        onRemoved={() => {
          void navigate({ href: `/${workspaceSlug}/projects/${projectId}/schedulers/list` });
        }}
      />
    </div>
  );
}

/** Route body: resolves the two standing reads, then hands over. */
export function BindingDetailPage({ workspaceSlug, projectId, bindingId }: BindingDetailPageProps): React.ReactElement {
  const { workspace } = useWorkspace(workspaceSlug);
  const roles = useProjectRoles(workspaceSlug);
  const membership = useWorkspaceMembership(workspaceSlug);
  const canManage = roles.isSuccess && isProjectAdmin(roles.data[projectId] ?? null);
  const workspaceAdmin = membership.isSuccess && (membership.data?.role ?? 0) >= ROLE_ADMIN;

  useDocumentHead({
    title: `${workspace?.name ?? workspaceSlug} · Scheduler install`,
    robots: "noindex, nofollow",
  });

  if (roles.isPending || membership.isPending) return <DetailSkeleton />;

  return (
    <BindingDetailBody
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      bindingId={bindingId}
      canManage={canManage}
      canSeeWorkspaceDefinition={workspaceAdmin}
    />
  );
}
