/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Bot, Circle, LayoutDashboard } from "lucide-react";
import { NavLink } from "react-router";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { RunnerService } from "@pi-dash/services";
import type { IRunner } from "@pi-dash/types";

const service = new RunnerService();

function statusColor(status: IRunner["status"]) {
  if (status === "online") return "text-success-primary";
  if (status === "busy") return "text-info-primary";
  if (status === "revoked") return "text-warning-primary";
  return "text-tertiary";
}

type RunnersSideNavProps = {
  workspaceId?: string;
  workspaceSlug?: string;
  /**
   * When set, the panel is project-scoped: the contact list is filtered to
   * runners whose pod belongs to this project and links point at the
   * project-scoped route tree. Omit for the workspace-wide aggregate.
   */
  projectId?: string;
};

/**
 * Left rail for the AI Workers panel: an Overview link plus one chat contact
 * per runner. Shared verbatim between the workspace-wide aggregate
 * (``/<workspace>/runners``) and the project-scoped panel
 * (``/<workspace>/projects/<projectId>/runners``) — the only difference is the
 * ``projectId`` scope, which narrows the list and rewrites the link base.
 */
export const RunnersSideNav = observer(function RunnersSideNav(props: RunnersSideNavProps) {
  const { workspaceId, workspaceSlug, projectId } = props;
  const { t } = useTranslation();

  const base = projectId ? `/${workspaceSlug}/projects/${projectId}/runners` : `/${workspaceSlug}/runners`;

  const { data: runners } = useSWR<IRunner[]>(
    workspaceId ? ["runners-middle-panel", workspaceId, projectId ?? null] : null,
    () => service.list(workspaceId, projectId),
    { refreshInterval: 5_000 }
  );

  const runnerList = runners ?? [];

  return (
    <aside className="w-[280px] shrink-0 border-r border-subtle bg-surface-1">
      <div className="flex h-12 items-center border-b border-subtle px-4 text-14 font-semibold text-primary">
        {t("AI Agents")}
      </div>
      <nav className="flex flex-col gap-1 p-2">
        <NavLink
          to={base}
          end
          className={({ isActive }) =>
            `flex h-9 items-center gap-2 rounded px-2 text-13 ${
              isActive ? "bg-layer-1 font-medium text-primary" : "text-secondary hover:bg-layer-1"
            }`
          }
        >
          <LayoutDashboard className="size-4" />
          <span>{t("Overview")}</span>
        </NavLink>
        <div className="mt-2 px-2 text-11 font-medium text-tertiary uppercase">{t("Runners")}</div>
        {runnerList.length === 0 ? (
          <div className="px-2 py-2 text-12 text-tertiary">
            {projectId ? t("No runners connected to this project yet.") : t("No runners connected yet.")}
          </div>
        ) : (
          runnerList.map((runner) => (
            <NavLink
              key={runner.id}
              to={`${base}/chat/${runner.id}`}
              className={({ isActive }) =>
                `flex min-h-11 items-center gap-2 rounded px-2 py-2 text-13 ${
                  isActive ? "bg-layer-1 font-medium text-primary" : "text-secondary hover:bg-layer-1"
                }`
              }
            >
              <Bot className="size-4 shrink-0" />
              <span className="min-w-0 flex-1 truncate">{runner.name}</span>
              <Circle className={`size-2 fill-current ${statusColor(runner.status)}`} />
            </NavLink>
          ))
        )}
      </nav>
    </aside>
  );
});
