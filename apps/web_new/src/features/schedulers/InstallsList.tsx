// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One project's installed schedulers (parity/specs/agents.md §2, AGT-007).
// The list renders for every project viewer; the inline enable switch is
// live for project admins and inert for everyone else, and the creation
// control only exists for project admins. A row opens the install detail
// while the switch and the action cell never navigate. The same panel
// backs the project tab and the project-settings variant.

import type { SchedulerBinding } from "@pidash/api-client";
import { Button, Skeleton, Switch } from "@pidash/kit";
import { CalendarClock, Plus } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../core/api/client.js";
import { useWorkspace } from "../../core/session/index.js";
import { useProjectRoles } from "../auth/index.js";
import { isProjectAdmin, projectDetailsQueryOptions } from "../projects/index.js";
import { useDocumentHead } from "../../shared/shell/index.js";
import { useSchedulerBindings, useToggleSchedulerBinding } from "./installData.js";
import { describeRrule } from "./schedule.js";

export interface InstallsListProps {
  workspaceSlug: string;
  projectId: string;
  /** Project-settings variant: heading and copy differ, gating does not. */
  variant?: "project" | "settings";
  /** Opens the install detail; the route owns navigation. */
  onOpenInstall: (binding: SchedulerBinding) => void;
  /** Opens the install/author dialog (project admins only). */
  onCreate: () => void;
  /** NEWFRONT-587 supplies these; without them the action cell is empty. */
  onEditInstall?: ((binding: SchedulerBinding) => void) | undefined;
  onUninstallInstall?: ((binding: SchedulerBinding) => void) | undefined;
}

const NEVER = "(never)";

export function formatRunTimestamp(value: string | null): string {
  if (value === null || value === "") return NEVER;
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return NEVER;
  return new Intl.DateTimeFormat("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(parsed);
}

/** Container time for the row; the rule's raw text is the cell's tooltip. */
export function scheduleText(binding: Pick<SchedulerBinding, "rrule">): string {
  return describeRrule(binding.rrule);
}

function InstallsSkeleton(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading installs" className="flex flex-col gap-(--space-3) p-(--space-6)">
      {[0, 1, 2].map((row) => (
        <div key={row} className="flex items-center gap-(--space-6)">
          <Skeleton width="180px" height="16px" />
          <Skeleton width="140px" height="16px" />
          <Skeleton width="90px" height="16px" />
        </div>
      ))}
    </div>
  );
}

export function InstallsList({
  workspaceSlug,
  projectId,
  variant = "project",
  onOpenInstall,
  onCreate,
  onEditInstall,
  onUninstallInstall,
}: InstallsListProps): React.ReactElement {
  const { workspace } = useWorkspace(workspaceSlug);
  const roles = useProjectRoles(workspaceSlug);
  const projects = useQuery(projectDetailsQueryOptions(getAppClient(), workspaceSlug));
  const bindings = useSchedulerBindings(workspaceSlug, projectId);
  const toggle = useToggleSchedulerBinding(workspaceSlug);
  // One in-flight flip at a time per row, so a double click cannot race
  // two writes against the same install.
  const [flipping, setFlipping] = React.useState<string | null>(null);

  const project = projects.data?.find((row) => row.id === projectId);
  const canManage = roles.isSuccess && isProjectAdmin(roles.data[projectId] ?? null);
  const actions: Array<{ key: "edit" | "uninstall"; label: string; run: (binding: SchedulerBinding) => void }> = [];
  if (canManage && onEditInstall !== undefined) {
    actions.push({ key: "edit", label: "Edit", run: onEditInstall });
  }
  if (canManage && onUninstallInstall !== undefined) {
    actions.push({ key: "uninstall", label: "Uninstall", run: onUninstallInstall });
  }
  const showActions = actions.length > 0;
  const loading = bindings.isPending || roles.isPending;
  const title = project?.name === undefined ? "Schedulers" : `${project.name} · Schedulers`;

  useDocumentHead({
    title: `${workspace?.name ?? workspaceSlug} · ${title}`,
    robots: "noindex, nofollow",
  });

  const setEnabled = (binding: SchedulerBinding, enabled: boolean): void => {
    setFlipping(binding.id);
    toggle.mutate({ projectId, bindingId: binding.id, enabled }, { onSettled: () => setFlipping(null) });
  };

  const intro =
    variant === "settings"
      ? "Scheduled instructions installed on this project. Each install fires its prompt on the schedule below."
      : "Scheduled agent runs installed on this project. Members read the list; project admins manage the installs.";

  return (
    <section aria-labelledby="project-installs-heading" className="flex h-full flex-col">
      <header className="flex items-start justify-between gap-(--space-6) px-(--space-6) pt-(--space-6)">
        <div>
          <h1 id="project-installs-heading" className="text-h3 text-(--text)">
            Schedulers
          </h1>
          <p className="text-body text-(--text-muted)">{intro}</p>
        </div>
        {canManage ? (
          <Button variant="primary" onClick={onCreate}>
            <Plus size={16} strokeWidth={1.5} aria-hidden="true" />
            New Scheduler
          </Button>
        ) : null}
      </header>

      {loading ? <InstallsSkeleton /> : null}

      {bindings.isError && !loading ? (
        <div role="alert" className="flex flex-col items-start gap-(--space-4) p-(--space-8)">
          <h2 className="text-h3 text-(--text)">Installs could not be loaded</h2>
          <p className="text-body text-(--text-muted)">{bindings.error.message}</p>
          <Button variant="primary" onClick={() => void bindings.refetch()}>
            Try again
          </Button>
        </div>
      ) : null}

      {bindings.isSuccess && !loading ? (
        <div className="overflow-auto px-(--space-6) pt-(--space-6) pb-(--space-8)">
          <table
            className="text-body w-full border-separate border-spacing-0 text-left"
            aria-label="Installed schedulers"
          >
            <thead>
              <tr className="text-meta text-(--text-muted)">
                {["Scheduler", "Schedule", "Next run", "Last run", "Status", "Updated", ""].map((label) => (
                  <th
                    key={label}
                    scope="col"
                    className="border-b border-(--border) px-(--space-3) py-(--space-3) font-medium"
                  >
                    {label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {bindings.data.length === 0 ? (
                <tr>
                  <td colSpan={7} className="px-(--space-4) py-(--space-12) text-center">
                    <p className="flex items-center justify-center gap-(--space-2) font-medium text-(--text)">
                      <CalendarClock size={16} strokeWidth={1.5} aria-hidden="true" />
                      No schedulers installed on this project yet
                    </p>
                    <p className="text-meta text-(--text-muted)">
                      {canManage
                        ? "Create one here, or install a definition from the workspace catalog."
                        : "A project admin can install scheduled work here."}
                    </p>
                  </td>
                </tr>
              ) : (
                bindings.data.map((binding) => {
                  const enabledLabel = binding.enabled ? "Enabled" : "Disabled";
                  const rowBusy = flipping === binding.id;
                  return (
                    <tr
                      key={binding.id}
                      data-install-row={binding.scheduler_slug}
                      className="cursor-pointer hover:bg-(--subtle)"
                      onClick={() => onOpenInstall(binding)}
                    >
                      <td className="border-b border-(--border) px-(--space-3) py-(--space-3)">
                        <div className="flex items-center gap-(--space-2) font-medium text-(--text)">
                          <span
                            aria-hidden="true"
                            className="inline-block size-2.5 shrink-0 rounded-sm"
                            style={{ backgroundColor: binding.scheduler_color }}
                          />
                          <span>{binding.scheduler_name}</span>
                        </div>
                        <code className="text-meta text-(--text-muted)">{binding.scheduler_slug}</code>
                      </td>
                      <td
                        className="border-b border-(--border) px-(--space-3) py-(--space-3) text-(--text-muted)"
                        title={binding.rrule}
                      >
                        {scheduleText(binding)}
                      </td>
                      <td className="border-b border-(--border) px-(--space-3) py-(--space-3) text-(--text-muted)">
                        {formatRunTimestamp(binding.next_run_at)}
                      </td>
                      <td className="border-b border-(--border) px-(--space-3) py-(--space-3) text-(--text-muted)">
                        {formatRunTimestamp(binding.last_run_ended_at)}
                      </td>
                      <td
                        className="border-b border-(--border) px-(--space-3) py-(--space-3)"
                        onClick={(event) => event.stopPropagation()}
                      >
                        <div className="flex items-center gap-(--space-2)">
                          <Switch
                            label={binding.enabled ? "Disable scheduler" : "Enable scheduler"}
                            checked={binding.enabled}
                            disabled={!canManage || rowBusy}
                            onCheckedChange={(next) => setEnabled(binding, next)}
                          />
                          <span className="text-meta text-(--text-muted)">{enabledLabel}</span>
                        </div>
                      </td>
                      <td className="border-b border-(--border) px-(--space-3) py-(--space-3) text-(--text-muted)">
                        {formatRunTimestamp(binding.updated_at)}
                      </td>
                      <td
                        className="border-b border-(--border) px-(--space-3) py-(--space-3)"
                        onClick={(event) => event.stopPropagation()}
                      >
                        {showActions ? (
                          <div className="flex items-center justify-end gap-(--space-2)">
                            {actions.map((action) => (
                              <Button
                                key={action.key}
                                size="small"
                                variant={action.key === "edit" ? "ghost" : "danger"}
                                onClick={() => action.run(binding)}
                              >
                                {action.label}
                              </Button>
                            ))}
                          </div>
                        ) : null}
                      </td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      ) : null}
    </section>
  );
}
