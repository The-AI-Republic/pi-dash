// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace layout. Thin: session and membership gates, prefetch, shell
// composition. Signed-out visitors bounce to sign-in with a return path;
// unknown workspaces render the access notice below.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, notFound, Outlet, redirect, useRouterState } from "@tanstack/react-router";
import type * as React from "react";

import { meQueryOptions, selectWorkspace, workspacesQueryOptions } from "../../core/session/queries.js";
import { useSignOut } from "../../features/auth/index.js";
import { projectsQueryOptions } from "../../features/projects/index.js";
import { useCommandPalette } from "../../shared/commands/index.js";
import { AppShell, Sidebar, TitleBar } from "../../shared/shell/index.js";

export const Route = createFileRoute("/$ws")({
  beforeLoad: async ({ context, params, location }) => {
    const me = await context.queryClient.ensureQueryData(meQueryOptions(context.apiClient)).catch(() => null);
    if (!me) {
      throw redirect({ to: "/sign-in", search: { next: location.href } });
    }
    const workspaces = await context.queryClient
      .ensureQueryData(workspacesQueryOptions(context.apiClient))
      .catch(() => []);
    if (!selectWorkspace(workspaces, params.ws)) {
      throw notFound();
    }
    return {};
  },
  loader: async ({ context, params }) => {
    await Promise.all([
      context.queryClient.ensureQueryData(workspacesQueryOptions(context.apiClient)),
      context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws)),
    ]);
  },
  notFoundComponent: function WorkspaceAccessNotice() {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-2) bg-(--bg) p-(--space-8)">
        <h1 className="text-h2 text-(--text)">Workspace not found</h1>
        <p className="text-body text-(--text-muted)">
          You may not belong to it, or the address is wrong. Check your invitations or the workspace list.
        </p>
      </main>
    );
  },
  component: function WorkspaceLayoutComponent() {
    const params = Route.useParams();
    const navigate = Route.useNavigate();
    const matches = useRouterState({ select: (state) => state.matches });
    const openPalette = useCommandPalette((state) => state.openPalette);
    const signOut = useSignOut();

    return (
      <WorkspaceShell
        workspaceSlug={params.ws}
        childProjectId={lastParam(matches, "projectId")}
        onSelectWorkspace={(slug) => navigate({ to: "/$ws", params: { ws: slug } })}
        onSelectProject={(projectId) =>
          navigate({ to: "/$ws/projects/$projectId/issues", params: { ws: params.ws, projectId }, search: {} })
        }
        onOpenPalette={openPalette}
        onSignOut={() => {
          void signOut.mutateAsync().then(() => navigate({ to: "/sign-in" }));
        }}
      />
    );
  },
});

function lastParam(matches: Array<{ params: Record<string, unknown> }>, name: string): string | null {
  for (let index = matches.length - 1; index >= 0; index--) {
    const value = matches[index]?.params[name];
    if (typeof value === "string") return value;
  }
  return null;
}

function WorkspaceShell({
  workspaceSlug,
  childProjectId,
  onSelectWorkspace,
  onSelectProject,
  onOpenPalette,
  onSignOut,
}: {
  workspaceSlug: string;
  childProjectId: string | null;
  onSelectWorkspace: (slug: string) => void;
  onSelectProject: (projectId: string) => void;
  onOpenPalette: () => void;
  onSignOut: () => void;
}): React.ReactElement {
  const context = Route.useRouteContext();
  const { data: me } = useSuspenseQuery(meQueryOptions(context.apiClient));
  const { data: workspaces } = useSuspenseQuery(workspacesQueryOptions(context.apiClient));
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(context.apiClient, workspaceSlug));

  const workspace = selectWorkspace(workspaces, workspaceSlug);
  const activeProject = projects.find((project) => project.id === childProjectId) ?? null;
  const userLabel = me.display_name || me.email;

  return (
    <AppShell
      commands={[
        {
          id: "shell.sign-out",
          title: "Sign out",
          hint: userLabel,
          section: "Account",
          run: onSignOut,
        },
        {
          id: "shell.go-home",
          title: "Go to workspace home",
          hint: workspace?.name ?? workspaceSlug,
          section: "Navigate",
          run: () => onSelectWorkspace(workspaceSlug),
        },
      ]}
      topBar={
        <TitleBar
          workspaceName={workspace?.name ?? workspaceSlug}
          section={activeProject?.name ?? ""}
          userLabel={userLabel}
          onOpenPalette={onOpenPalette}
        />
      }
      sidebar={
        <Sidebar
          workspaces={workspaces.map((entry) => ({ id: entry.id, slug: entry.slug, name: entry.name }))}
          activeSlug={workspaceSlug}
          onSelectWorkspace={onSelectWorkspace}
          projects={projects.map((project) => ({ id: project.id, name: project.name, identifier: project.identifier }))}
          activeProjectId={activeProject?.id ?? null}
          onSelectProject={onSelectProject}
          userLabel={userLabel}
          onSignOut={onSignOut}
        />
      }
    >
      <Outlet />
    </AppShell>
  );
}
