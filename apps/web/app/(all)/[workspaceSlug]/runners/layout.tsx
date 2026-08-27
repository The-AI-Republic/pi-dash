/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Bot, Circle, LayoutDashboard } from "lucide-react";
import { NavLink, Outlet, useParams } from "react-router";
import useSWR from "swr";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { RunnerService } from "@pi-dash/services";
import type { IRunner } from "@pi-dash/types";
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { ProjectsAppPowerKProvider } from "@/components/power-k/projects-app-provider";
import { WorkspaceShell } from "@/components/workspace/workspace-shell";
import { useUserPermissions } from "@/hooks/store/user";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();

function statusColor(status: IRunner["status"]) {
  if (status === "online") return "text-success-primary";
  if (status === "busy") return "text-info-primary";
  if (status === "revoked") return "text-warning-primary";
  return "text-tertiary";
}

const RunnersLayout = observer(function RunnersLayout() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const { currentWorkspace } = useWorkspace();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();
  const { t } = useTranslation();
  const workspaceId = currentWorkspace?.id;

  const canViewRunners = allowPermissions(
    [EUserPermissions.ADMIN, EUserPermissions.MEMBER],
    EUserPermissionsLevel.WORKSPACE
  );

  const { data: runners } = useSWR<IRunner[]>(
    workspaceId ? ["runners-middle-panel", workspaceId] : null,
    () => service.list(workspaceId),
    { refreshInterval: 5_000 }
  );

  if (workspaceUserInfo && !canViewRunners) {
    return (
      <>
        {/* Mounts the workspace command palette + shared modal hosts (incl. the
            "New work item" create modal) — runners sits outside the (projects)
            layout, so without this the sidebar "New work item" button is inert. */}
        <ProjectsAppPowerKProvider />
        <WorkspaceShell>
          <NotAuthorizedView section="general" className="h-auto" />
        </WorkspaceShell>
      </>
    );
  }

  const base = `/${workspaceSlug}/runners`;

  return (
    <>
      {/* Mounts the workspace command palette + shared modal hosts (incl. the
          "New work item" create modal) — runners sits outside the (projects)
          layout, so without this the sidebar "New work item" button is inert. */}
      <ProjectsAppPowerKProvider />
      <WorkspaceShell>
        <div className="flex h-full w-full overflow-hidden">
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
                <span>Overview</span>
              </NavLink>
              <div className="mt-2 px-2 text-11 font-medium text-tertiary uppercase">Runners</div>
              {(runners ?? []).map((runner) => (
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
              ))}
            </nav>
          </aside>
          {/* Pages pad themselves (p-6) so full-bleed surfaces like the runner
              chat can span edge-to-edge. */}
          <main className="min-w-0 flex-1 overflow-auto">
            <Outlet />
          </main>
        </div>
      </WorkspaceShell>
    </>
  );
});

export default RunnersLayout;
