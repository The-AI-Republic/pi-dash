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

import { useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Outlet, redirect, useRouterState } from "@tanstack/react-router";
import { LayoutGrid } from "lucide-react";
import * as React from "react";

import {
  edition,
  isAppRailEnabled,
  isSidebarToggleEnabled,
  resolveEditionSidebarHref,
} from "../../../core/edition/index.js";
import { platform } from "../../../core/platform/index.js";
import {
  CREATE_WORKSPACE_PATH,
  decidePageGate,
  instanceConfigQueryOptions,
  isOnboardedProfile,
  loadGateInputs,
  meQueryOptions,
  ONBOARDING_PATH,
  ROLE_GUEST,
  selectWorkspace,
  selectWorkspaceRole,
  sessionKeys,
  usePermissions,
  workspacesQueryOptions,
} from "../../../core/session/index.js";
import { runSignOutAction, signOutCommand, useSignOut, useUpdateProfile } from "../../../features/auth/index.js";
import { QuickCreateButton } from "../../../features/issues/index.js";
import { unreadNotificationsQueryOptions } from "../../../features/notifications/index.js";
import { projectsQueryOptions, selectSidebarProjects, SidebarProjectsGroup } from "../../../features/projects/index.js";
import {
  SidebarFavoritesGroup,
  SidebarMoreGroup,
  favoritesQueryOptions,
  selectPersonalEntries,
  selectPersonalGroup,
  selectVisiblePersonalEntries,
  sidebarPreferencesQueryOptions,
  useSidebarPreferences,
} from "../../../features/sidebar/index.js";
import type { SearchResultHit } from "../../../shared/commands/index.js";
import {
  APP_VERSION,
  AppShell,
  getToastManager,
  HelpMenu,
  isNotificationsPath,
  isRailUndocked,
  resolveFileUrl,
  Sidebar,
  TopBar,
  TopBarSearch,
  UserMenu,
  useNarrowViewport,
  useRailState,
  useSessionExpiredRedirect,
  useShellChrome,
  useShortcutsDialog,
  WorkspaceSwitcher,
  type RailDockItem,
  type SwitcherWorkspace,
} from "../../../shared/shell/index.js";
import { isWorkspaceSettingsRouteId } from "../../../features/workspace-settings/index.js";
import { ToastHost } from "@pidash/kit";
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
// The rail mounts only on rail-enabled editions, so it rides a lazy chunk
// instead of the initial-JS chain (size budget); AppShell suspends around
// the rail slot while it loads.
const WorkspaceRail = React.lazy(() =>
  import("../../../shared/shell/WorkspaceRail.js").then((module) => ({ default: module.WorkspaceRail }))
);
// The personalization dialog splits off the initial bundle (size gate):
// it only renders (and loads) once opened from the sidebar header
// (shell spec §6, SHELL-070/071).
const PersonalizeDialog = React.lazy(() =>
  import("../../../features/sidebar/components/PersonalizeDialog.js").then((module) => ({
    default: module.PersonalizeDialog,
  }))
);
// The product-updates dialog splits off the same way (shell spec §4,
// SHELL-060): it only renders (and loads) once opened from the help
// menu.
const LazyUpdatesDialog = React.lazy(() =>
  import("../../../shared/shell/UpdatesDialog.js").then((module) => ({ default: module.UpdatesDialog }))
);
// The agent-runtime supervisor splits off the same way (RUN-044): it is a
// background desktop service, so it loads right after first paint instead
// of weighing the initial chain.
const LazyRuntimeSupervisor = React.lazy(() =>
  import("../../../features/runners/index.js").then((module) => ({ default: module.RuntimeSupervisor }))
);
// The desktop update control rides a lazy chunk (DESK-018, size gate): it
// does nothing on web builds, so the web initial chain never carries it.
const LazyUpdateControl = React.lazy(() =>
  import("../../../features/updater/index.js").then((module) => ({ default: module.DesktopUpdateControl }))
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
    // Projects start loading but never block the shell: the sidebar
    // renders placeholder rows while the collection resolves (SHELL-049).
    void context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws)).catch(() => undefined);
    // Favorites likewise never block: the section hides while they load
    // or when they fail (SHELL-056), and waiting here would stall a
    // cached desktop start behind the query's retries.
    void context.queryClient
      .ensureQueryData(favoritesQueryOptions(context.apiClient, params.ws))
      .catch(() => undefined);
    await Promise.all([
      context.queryClient.ensureQueryData(workspacesQueryOptions(context.apiClient)),
      context.queryClient
        .ensureQueryData(unreadNotificationsQueryOptions(context.apiClient, params.ws))
        // The unread dot is optional chrome: a failing count must not fail the shell.
        .catch(() => undefined),
      context.queryClient
        .ensureQueryData(sidebarPreferencesQueryOptions(context.apiClient, params.ws))
        // Personal entries are optional chrome too: the sidebar falls
        // back to visible defaults while they load or when they fail.
        .catch(() => undefined),
    ]);
  },
  component: function GatedWorkspaceLayoutComponent() {
    const params = Route.useParams();
    const navigate = Route.useNavigate();
    const matches = useRouterState({ select: (state) => state.matches });
    const pathname = useRouterState({ select: (state) => state.location.pathname });
    const signOut = useSignOut();
    // Workspace settings replace the ordinary workspace chrome with their
    // own frame (WSET-001/002): the layout still runs its auth gate, then
    // hands the window to the settings shell or its denial view. The
    // match decides this, not the pathname, so sibling areas under
    // `settings/` (project settings) keep the chrome. The background
    // agent-runtime supervisor (RUN-044) stays mounted on desktop: it is
    // not chrome and settings add no desktop-only branch (WSET-040).
    if (matches.some((match) => isWorkspaceSettingsRouteId(match.routeId))) {
      return (
        <>
          <React.Suspense fallback={null}>
            <LazyRuntimeSupervisor />
          </React.Suspense>
          {/* Settings replace AppShell's chrome, not its workspace
              wirings: an expired session leaves them like any signed-in
              page and notices keep the one ToastHost. */}
          <SettingsSurfaceChrome />
          <React.Suspense fallback={null}>
            <Outlet />
          </React.Suspense>
        </>
      );
    }

    return (
      <>
        {/* Background agent-runtime supervisor (RUN-044): desktop only, renders nothing on web. */}
        <React.Suspense fallback={null}>
          <LazyRuntimeSupervisor />
        </React.Suspense>
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
      </>
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

/** App-shell wirings the settings pages would otherwise lose. */
function SettingsSurfaceChrome(): React.ReactElement {
  useSessionExpiredRedirect();
  return <ToastHost manager={getToastManager()} />;
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
  const queryClient = useQueryClient();
  const { data: me } = useSuspenseQuery(meQueryOptions(context.apiClient));
  const { data: workspaces } = useSuspenseQuery(workspacesQueryOptions(context.apiClient));
  const projectsQuery = useQuery(projectsQueryOptions(context.apiClient, workspaceSlug));
  const projects = projectsQuery.data ?? [];
  const [deactivateOpen, setDeactivateOpen] = React.useState(false);
  const [createProjectOpen, setCreateProjectOpen] = React.useState(false);
  const [personalizeOpen, setPersonalizeOpen] = React.useState(false);
  const [updatesOpen, setUpdatesOpen] = React.useState(false);
  const { data: unread } = useQuery(unreadNotificationsQueryOptions(context.apiClient, workspaceSlug));
  const { data: instanceConfig } = useQuery(instanceConfigQueryOptions(context.apiClient));
  const sidebarPrefs = useSidebarPreferences(workspaceSlug);
  const { canEdit, role } = usePermissions(workspaceSlug);
  const sidebarCollapsed = useShellChrome((state) => state.sidebarCollapsed);
  const clearPeek = useShellChrome((state) => state.clearPeek);
  const setSidebarCollapsed = useShellChrome((state) => state.setSidebarCollapsed);
  const toggleSidebarCollapsed = useShellChrome((state) => state.toggleSidebarCollapsed);
  const undocked = useRailState((state) => isRailUndocked(state, workspaceSlug));
  const narrow = useNarrowViewport();
  const stampLastWorkspace = useUpdateProfile();

  const workspace = selectWorkspace(workspaces, workspaceSlug);
  const activeProject = projects.find((project) => project.id === childProjectId) ?? null;
  const userLabel = me.display_name || me.email;
  const userRole = selectWorkspaceRole(workspaces, workspaceSlug) ?? ROLE_GUEST;
  const sidebarHidden = isNotificationsPath(currentPath);
  const apiBaseUrl = context.apiClient.baseUrl;
  const switcherWorkspaces: SwitcherWorkspace[] = workspaces.map((entry) => ({
    id: entry.id,
    slug: entry.slug,
    name: entry.name,
    role: entry.role,
    totalMembers: entry.total_members,
    logoSrc: resolveFileUrl(entry.logo_url, apiBaseUrl),
  }));

  // Narrow autohide (SHELL-062): every shell navigation collapses the
  // floating sidebar. Same-address taps inside the sidebar collapse
  // through the shell's own tap capture instead.
  const collapseIfNarrow = (): void => {
    if (narrow) {
      clearPeek();
      setSidebarCollapsed(true);
    }
  };

  // Workspace switching (SHELL-058): collapse the floating sidebar,
  // remember the workspace on the profile so the next entry lands
  // there, refresh the landing reads, then navigate. A failed stamp
  // never blocks the navigation.
  const handleSelectWorkspace = (slug: string): void => {
    collapseIfNarrow();
    const target = selectWorkspace(workspaces, slug);
    if (target === undefined) {
      onSelectWorkspace(slug);
      return;
    }
    void stampLastWorkspace.mutateAsync({ last_workspace_id: target.id }).then(
      () => {
        void queryClient.invalidateQueries({ queryKey: sessionKeys.settings });
        onSelectWorkspace(slug);
      },
      () => {
        onSelectWorkspace(slug);
      }
    );
  };

  const handleSelectProject = (projectId: string): void => {
    collapseIfNarrow();
    onSelectProject(projectId);
  };

  const handleSelectSearchResult = (hit: SearchResultHit): void => {
    collapseIfNarrow();
    onSelectSearchResult(hit);
  };

  const handleNavigateHref = (href: string): void => {
    collapseIfNarrow();
    onNavigateHref(href);
  };
  const personalStates = selectPersonalEntries(sidebarPrefs.data);
  const personalGroup = selectPersonalGroup(sidebarPrefs.status, sidebarPrefs.data);
  // Member-only rows (drafts, creation) hide from guests (SHELL-046);
  // the quick-create picker files under joined projects only (SHELL-047).
  const personalEntries = canEdit ? selectVisiblePersonalEntries(personalStates, workspaceSlug) : [];
  const joinedProjects = selectSidebarProjects(projects);
  // Edition sidebar entries (CLOUD-021): workspace-relative targets
  // resolved against the active slug. Rendered for every role — guests
  // included — since installations belong to the viewer. The OSS
  // edition contributes none, so its sidebar is unchanged.
  const editionEntries = (edition.sidebar ?? []).map((entry) => ({
    key: entry.id,
    label: entry.label,
    href: resolveEditionSidebarHref(workspaceSlug, entry.to),
  }));
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

  const switcherShortcuts = {
    creationDisabled: instanceConfig?.is_workspace_creation_disabled ?? false,
    onSelectWorkspace: handleSelectWorkspace,
    onOpenSettings: (slug: string) => handleNavigateHref(`/${slug}/settings`),
    onInviteMembers: (slug: string) => handleNavigateHref(`/${slug}/settings/members`),
    onCreateWorkspace: () => handleNavigateHref(CREATE_WORKSPACE_PATH),
    onWorkspaceInvites: () => handleNavigateHref("/invitations"),
    onSignOut,
  };
  const accountShortcuts = {
    onYourWork: () => handleNavigateHref(`/${workspaceSlug}/profile/${me.id}`),
    onSettings: () => handleNavigateHref("/settings/profile/general"),
    onSignOut,
  };

  return (
    <AppShell
      commands={[signOutCommand({ userLabel, onSignOut })]}
      searchScope={searchScope}
      onSelectSearchResult={handleSelectSearchResult}
      topBar={
        <TopBar
          workspaceSlug={workspaceSlug}
          section={activeProject?.name ?? ""}
          currentPath={currentPath}
          sidebarMounted={!sidebarHidden}
          sidebarCollapsed={sidebarCollapsed}
          sidebarToggleEnabled={isSidebarToggleEnabled(edition)}
          onToggleSidebar={() => {
            clearPeek();
            toggleSidebarCollapsed();
          }}
          unreadTotal={unread?.total_unread_notifications_count ?? 0}
          unreadMentions={unread?.mention_unread_notifications_count ?? 0}
          searchBox={<TopBarSearch scope={searchScope} onSelectSearchResult={handleSelectSearchResult} />}
          workspaceMenu={
            <WorkspaceSwitcher
              workspaces={switcherWorkspaces}
              activeSlug={workspaceSlug}
              mount="topbar"
              {...switcherShortcuts}
            />
          }
          helpMenu={
            <HelpMenu
              version={APP_VERSION}
              onOpenShortcuts={() => useShortcutsDialog.getState().openShortcuts()}
              onOpenUpdates={() => setUpdatesOpen(true)}
              onOpenExternal={(url: string) => void platform.openExternal(url)}
            />
          }
          accountMenu={
            <UserMenu
              displayName={userLabel}
              email={me.email}
              avatarSrc={resolveFileUrl(me.avatar_url, apiBaseUrl)}
              role={userRole}
              variant="compact"
              onDeactivateAccount={() => setDeactivateOpen(true)}
              {...accountShortcuts}
            />
          }
        />
      }
      sidebar={
        sidebarHidden ? null : (
          <Sidebar
            workspaces={switcherWorkspaces}
            activeSlug={workspaceSlug}
            userLabel={userLabel}
            userEmail={me.email}
            userAvatarSrc={resolveFileUrl(me.avatar_url, apiBaseUrl)}
            userRole={userRole}
            onYourWork={accountShortcuts.onYourWork}
            onUserSettings={accountShortcuts.onSettings}
            onDeactivateAccount={() => setDeactivateOpen(true)}
            personalEntries={personalEntries}
            editionEntries={editionEntries}
            currentPath={currentPath}
            onPersonalize={() => setPersonalizeOpen(true)}
            quickActions={
              <QuickCreateButton
                workspaceSlug={workspaceSlug}
                projects={joinedProjects.map((project) => ({
                  id: project.id,
                  name: project.name,
                  memberRole: project.member_role,
                  archived: project.archived_at !== null,
                }))}
                canCreate={canEdit}
              />
            }
            projectsGroup={
              <SidebarProjectsGroup
                workspaceSlug={workspaceSlug}
                projects={projects}
                projectsStatus={projectsQuery.status}
                currentPath={currentPath}
                activeProjectId={activeProject?.id ?? null}
                canCreateProject={canEdit}
                onCreateProject={() => setCreateProjectOpen(true)}
                onNavigate={handleNavigateHref}
              />
            }
            moreGroup={<SidebarMoreGroup workspaceSlug={workspaceSlug} role={role} currentPath={currentPath} />}
            favoritesGroup={
              // Favorites are admin and member only; guests never see
              // the section (SHELL-056).
              canEdit ? <SidebarFavoritesGroup workspaceSlug={workspaceSlug} /> : undefined
            }
            // The desktop updater fills the shell's sidebar slot
            // (SHELL-102); it renders nothing on web and with nothing
            // waiting (DESK-018).
            updateControl={
              <React.Suspense fallback={null}>
                <LazyUpdateControl />
              </React.Suspense>
            }
            onNavigateHref={handleNavigateHref}
            {...switcherShortcuts}
          />
        )
      }
      rail={rail}
      dialogs={
        <>
          {personalizeOpen ? (
            <React.Suspense fallback={null}>
              <PersonalizeDialog
                open={personalizeOpen}
                onOpenChange={setPersonalizeOpen}
                workspaceSlug={workspaceSlug}
                personal={personalGroup}
              />
            </React.Suspense>
          ) : null}
          {updatesOpen ? (
            <React.Suspense fallback={null}>
              <LazyUpdatesDialog
                open={updatesOpen}
                onOpenChange={setUpdatesOpen}
                version={APP_VERSION}
                changelogUrl={instanceConfig?.instance_changelog_url ?? null}
                selfManaged={instanceConfig?.is_self_managed ?? false}
                onOpenExternal={(url: string) => void platform.openExternal(url)}
              />
            </React.Suspense>
          ) : null}
        </>
      }
    >
      {deactivateOpen ? (
        <React.Suspense fallback={null}>
          <LazyDeactivateAccountDialog open={deactivateOpen} onOpenChange={setDeactivateOpen} email={me.email} />
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
          onCreateWorkspace={() => handleNavigateHref(CREATE_WORKSPACE_PATH)}
          onNavigate={handleNavigateHref}
          onWorkspaceInvites={() => handleNavigateHref("/invitations")}
          onOpenExternal={(url: string) => void platform.openExternal(url)}
          onSelectWorkspace={handleSelectWorkspace}
          onSelectProject={handleSelectProject}
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
