// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Gated workspace layout (pathless). Thin: auth gate, prefetch, shell
// composition. Signed-out visitors bounce to the entry route with a
// return path, unfinished members funnel to onboarding, and only onboarded
// members render. The workspace catch-all (`/$ws/$`) is not a child of
// this layout, so unknown addresses render the illustrated not-found page
// with or without a session. Top frame chrome (SHELL-063-066, 099-102,
// from parity/specs/shell.md §5): the top bar, the collapsible sidebar
// and the edition-gated rail compose here; the palette search box and
// the menu mounts arrive with their owning slices.

import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Outlet, redirect, useRouterState } from "@tanstack/react-router";
import { LayoutGrid } from "lucide-react";
import * as React from "react";

import { edition, isAppRailEnabled, isSidebarToggleEnabled } from "../../../core/edition/index.js";
import { platform } from "../../../core/platform/index.js";
import {
  CREATE_WORKSPACE_PATH,
  decidePageGate,
  isOnboardedProfile,
  loadGateInputs,
  meQueryOptions,
  ONBOARDING_PATH,
  selectWorkspace,
  workspacesQueryOptions,
} from "../../../core/session/index.js";
import { runSignOutAction, signOutCommand, useSignOut } from "../../../features/auth/index.js";
import { unreadNotificationsQueryOptions } from "../../../features/notifications/index.js";
import { projectsQueryOptions } from "../../../features/projects/index.js";
import type { SearchResultHit } from "../../../shared/commands/index.js";
import {
  AppShell,
  isRailUndocked,
  Sidebar,
  TopBar,
  TopBarSearch,
  useRailState,
  useShellChrome,
  WorkspaceRail,
  type RailDockItem,
} from "../../../shared/shell/index.js";
import { resolveSearchTarget } from "./searchTarget.js";

// Deferred below the initial shell chain (Quality gates budget): the
// palette catalogs register once their chunk lands, and the creation
// dialog loads on first open (shell spec §8, SHELL-086-089).
const WorkspacePaletteEntries = React.lazy(() =>
  import("../../../shared/shell/entries/WorkspaceEntries.js").then((module) => ({
    default: module.WorkspacePaletteEntries,
  }))
);
const ProjectCreateEntry = React.lazy(() =>
  import("../../../features/projects/components/ProjectCreateEntry.js").then((module) => ({
    default: module.ProjectCreateEntry,
  }))
);
// The deactivate dialog splits off the initial bundle (size gate): it only
// renders (and loads) once the user opens it from the account menu.
const LazyDeactivateAccountDialog = React.lazy(() =>
  import("../../../features/auth/components/DeactivateAccountDialog.js").then((module) => ({
    default: module.DeactivateAccountDialog,
  }))
);

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
      context.queryClient.ensureQueryData(unreadNotificationsQueryOptions(context.apiClient, params.ws)),
    ]);
  },
  component: function GatedWorkspaceLayoutComponent() {
    const params = Route.useParams();
    const navigate = Route.useNavigate();
    const matches = useRouterState({ select: (state) => state.matches });
    const pathname = useRouterState({ select: (state) => state.location.pathname });
    const signOut = useSignOut();

    return (
      <WorkspaceShell
        workspaceSlug={params.ws}
        currentPath={pathname}
        childProjectId={lastParam(matches, "projectId")}
        onSelectWorkspace={(slug) => navigate({ to: "/$ws", params: { ws: slug } })}
        onSelectProject={(projectId) =>
          navigate({ to: "/$ws/projects/$projectId/issues", params: { ws: params.ws, projectId }, search: {} })
        }
        onSelectSearchResult={(hit: SearchResultHit) => navigate(resolveSearchTarget(hit, params.ws))}
        onNavigateHref={(href: string) => navigate({ href })}
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
  currentPath,
  childProjectId,
  onSelectWorkspace,
  onSelectProject,
  onSelectSearchResult,
  onNavigateHref,
  onSignOut,
}: {
  workspaceSlug: string;
  currentPath: string;
  childProjectId: string | null;
  onSelectWorkspace: (slug: string) => void;
  onSelectProject: (projectId: string) => void;
  onSelectSearchResult: (hit: SearchResultHit) => void;
  onNavigateHref: (href: string) => void;
  onSignOut: () => void;
}): React.ReactElement {
  const context = Route.useRouteContext();
  const { data: me } = useSuspenseQuery(meQueryOptions(context.apiClient));
  const { data: workspaces } = useSuspenseQuery(workspacesQueryOptions(context.apiClient));
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(context.apiClient, workspaceSlug));
  const [deactivateOpen, setDeactivateOpen] = React.useState(false);
  const [createProjectOpen, setCreateProjectOpen] = React.useState(false);
  const { data: unread } = useQuery(unreadNotificationsQueryOptions(context.apiClient, workspaceSlug));
  const sidebarCollapsed = useShellChrome((state) => state.sidebarCollapsed);
  const clearPeek = useShellChrome((state) => state.clearPeek);
  const toggleSidebarCollapsed = useShellChrome((state) => state.toggleSidebarCollapsed);
  const undocked = useRailState((state) => isRailUndocked(state, workspaceSlug));

  const workspace = selectWorkspace(workspaces, workspaceSlug);
  const activeProject = projects.find((project) => project.id === childProjectId) ?? null;
  const userLabel = me.display_name || me.email;
  const dockItems: RailDockItem[] = (edition.rail ?? []).map((entry) => ({
    id: entry.id,
    label: entry.label,
    to: entry.to,
    icon: <LayoutGrid size={20} strokeWidth={1.5} aria-hidden="true" />,
  }));
  const rail =
    isAppRailEnabled(edition) && !undocked ? (
      <WorkspaceRail workspaceSlug={workspaceSlug} currentPath={currentPath} dockItems={dockItems} />
    ) : undefined;

  const searchScope = { workspaceSlug, projectId: childProjectId };

  return (
    <AppShell
      commands={[signOutCommand({ userLabel, onSignOut })]}
      searchScope={searchScope}
      onSelectSearchResult={onSelectSearchResult}
      topBar={
        <TopBar
          workspaceSlug={workspaceSlug}
          section={activeProject?.name ?? ""}
          currentPath={currentPath}
          sidebarCollapsed={sidebarCollapsed}
          sidebarToggleEnabled={isSidebarToggleEnabled(edition)}
          onToggleSidebar={() => {
            clearPeek();
            toggleSidebarCollapsed();
          }}
          unreadTotal={unread?.total_unread_notifications_count ?? 0}
          unreadMentions={unread?.mention_unread_notifications_count ?? 0}
          searchBox={<TopBarSearch scope={searchScope} onSelectSearchResult={onSelectSearchResult} />}
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
          collapsed={sidebarCollapsed}
        />
      }
      rail={rail}
    >
      {deactivateOpen ? (
        <React.Suspense fallback={null}>
          <LazyDeactivateAccountDialog
            open={deactivateOpen}
            onOpenChange={setDeactivateOpen}
            email={me.email}
          />
        </React.Suspense>
      ) : null}
      <React.Suspense fallback={null}>
        <WorkspacePaletteEntries
          workspaceSlug={workspaceSlug}
          workspaceName={workspace?.name ?? workspaceSlug}
          project={
            activeProject === null
              ? null
              : {
                  id: activeProject.id,
                  name: activeProject.name,
                  views: {
                    cycleView: activeProject.cycle_view,
                    moduleView: activeProject.module_view,
                    issueViewsView: activeProject.issue_views_view,
                    pageView: activeProject.page_view,
                    inboxView: activeProject.inbox_view,
                  },
                }
          }
          projects={projects.map((project) => ({
            id: project.id,
            name: project.name,
            identifier: project.identifier,
          }))}
          workspaces={workspaces.map((entry) => ({ id: entry.id, name: entry.name, slug: entry.slug }))}
          onOpenCreateProject={() => setCreateProjectOpen(true)}
          onCreateWorkspace={() => onNavigateHref(CREATE_WORKSPACE_PATH)}
          onNavigate={onNavigateHref}
          onWorkspaceInvites={() => onNavigateHref("/invitations")}
          onOpenExternal={(url: string) => void platform.openExternal(url)}
          onSelectWorkspace={onSelectWorkspace}
          onSelectProject={onSelectProject}
        />
        {createProjectOpen ? (
          <ProjectCreateEntry
            workspaceSlug={workspaceSlug}
            open={createProjectOpen}
            onOpenChange={setCreateProjectOpen}
          />
        ) : null}
      </React.Suspense>
      <Outlet />
    </AppShell>
  );
}
