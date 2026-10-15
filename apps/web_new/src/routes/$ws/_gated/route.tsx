// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Gated workspace layout (pathless). Thin: auth gate, prefetch, shell
// composition. Signed-out visitors bounce to the entry route with a
// return path, unfinished members funnel to onboarding, and only onboarded
// members render. The workspace catch-all (`/$ws/$`) is not a child of
// this layout, so unknown addresses render the illustrated not-found page
// with or without a session.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Outlet, redirect, useRouterState } from "@tanstack/react-router";
import type * as React from "react";
import { useState } from "react";

import {
  decidePageGate,
  isOnboardedProfile,
  loadGateInputs,
  meQueryOptions,
  ONBOARDING_PATH,
  selectWorkspace,
  workspacesQueryOptions,
} from "../../../core/session/index.js";
import {
  DeactivateAccountDialog,
  runSignOutAction,
  signOutCommand,
  useSignOut,
} from "../../../features/auth/index.js";
import { projectsQueryOptions } from "../../../features/projects/index.js";
import type { SearchResultHit } from "../../../shared/commands/index.js";
import { AppShell, Sidebar, TitleBar } from "../../../shared/shell/index.js";
import { resolveSearchTarget } from "./searchTarget.js";

export const Route = createFileRoute("/$ws/_gated")({
  beforeLoad: async ({ context, location }) => {
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "signed-in-only",
      signedIn: me !== null,
      onboarded: isOnboardedProfile(profile),
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "sign-in") {
      // Land on the entry route, which renders the sign-in card in place
      // (SHELL-001): unknown visitors never reach workspace content, and
      // the return path survives so sign-in continues where they started.
      throw redirect({ to: "/", search: { next_path: decision.nextPath } });
    }
    if (decision.action === "onboarding") {
      throw redirect({ href: ONBOARDING_PATH });
    }
    return {};
  },
  loader: async ({ context, params }) => {
    await Promise.all([
      context.queryClient.ensureQueryData(workspacesQueryOptions(context.apiClient)),
      context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws)),
    ]);
  },
  component: function GatedWorkspaceLayoutComponent() {
    const params = Route.useParams();
    const navigate = Route.useNavigate();
    const matches = useRouterState({ select: (state) => state.matches });
    const signOut = useSignOut();

    return (
      <WorkspaceShell
        workspaceSlug={params.ws}
        childProjectId={lastParam(matches, "projectId")}
        onSelectWorkspace={(slug) => navigate({ to: "/$ws", params: { ws: slug } })}
        onSelectProject={(projectId) =>
          navigate({ to: "/$ws/projects/$projectId/issues", params: { ws: params.ws, projectId }, search: {} })
        }
        onSelectSearchResult={(hit: SearchResultHit) => navigate(resolveSearchTarget(hit, params.ws))}
        onSignOut={() => {
          void runSignOutAction(
            () => signOut.mutateAsync(),
            (landing) => {
              void navigate({ href: landing });
            }
          );
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
  onSelectSearchResult,
  onSignOut,
}: {
  workspaceSlug: string;
  childProjectId: string | null;
  onSelectWorkspace: (slug: string) => void;
  onSelectProject: (projectId: string) => void;
  onSelectSearchResult: (hit: SearchResultHit) => void;
  onSignOut: () => void;
}): React.ReactElement {
  const context = Route.useRouteContext();
  const { data: me } = useSuspenseQuery(meQueryOptions(context.apiClient));
  const { data: workspaces } = useSuspenseQuery(workspacesQueryOptions(context.apiClient));
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(context.apiClient, workspaceSlug));
  const [deactivateOpen, setDeactivateOpen] = useState(false);

  const workspace = selectWorkspace(workspaces, workspaceSlug);
  const activeProject = projects.find((project) => project.id === childProjectId) ?? null;
  const userLabel = me.display_name || me.email;

  const searchScope = { workspaceSlug, projectId: childProjectId };

  return (
    <AppShell
      commands={[
        signOutCommand({ userLabel, onSignOut }),
        {
          id: "shell.go-home",
          title: "Go to workspace home",
          hint: workspace?.name ?? workspaceSlug,
          section: "Navigation",
          run: () => onSelectWorkspace(workspaceSlug),
        },
      ]}
      searchScope={searchScope}
      onSelectSearchResult={onSelectSearchResult}
      topBar={
        <TitleBar
          workspaceName={workspace?.name ?? workspaceSlug}
          section={activeProject?.name ?? ""}
          userLabel={userLabel}
          searchScope={searchScope}
          onSelectSearchResult={onSelectSearchResult}
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
          onDeactivateAccount={() => setDeactivateOpen(true)}
        />
      }
    >
      <DeactivateAccountDialog open={deactivateOpen} onOpenChange={setDeactivateOpen} email={me.email} />
      <Outlet />
    </AppShell>
  );
}
