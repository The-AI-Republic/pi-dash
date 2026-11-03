// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-project settings shell (PSET-001, PSET-003, PSET-004; from
// parity/specs/project-settings-shell.md). The shell owns the project
// gate, the fixed sidebar, the narrow-screen drawer and the per-tab page
// chrome; each tab route wraps its body in the same shell and the tab
// gate so a direct address obeys the matrix and keeps the rail beside
// its refusal. The gate renders nothing while the detail read is in
// flight, then the content or a join, no-access or not-found view.

import { isApiError } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import { useRouterState } from "@tanstack/react-router";
import { Menu } from "lucide-react";
import * as React from "react";

import { ROLE_ADMIN } from "../../../core/session/permissions.js";
import {
  decideProjectDenial,
  useJoinProject,
  useProjectRoles,
  useProjectRow,
  useWorkspaceMembership,
} from "../../auth/index.js";
import { useNarrowViewport } from "../../../shared/shell/index.js";
import { ProjectSettingsNotAuthorized, ProjectSettingsRestriction } from "./ProjectSettingsRestriction.js";
import { ProjectSettingsSidebar } from "./ProjectSettingsSidebar.js";
import {
  decidePageGate,
  effectiveProjectRole,
  findTabByKey,
  findTabBySuffix,
  type ProjectRole,
  type ProjectSettingsTabKey,
} from "../tabs.js";

/** Row facts the header needs once the project read resolves. */
export interface ProjectSettingsProjectFacts {
  name: string;
  logoProps: Record<string, unknown>;
  identifier: string;
}

export interface ProjectSettingsContextValue {
  workspaceSlug: string;
  projectId: string;
  /**
   * The caller's role in this project with workspace-admin escalation
   * applied (PSET-004); null outside a membership.
   */
  role: ProjectRole;
  /** Whether the caller is a workspace admin (denial variants only). */
  workspaceAdmin: boolean;
  /** Header facts; null before the shell resolves the project. */
  project: ProjectSettingsProjectFacts | null;
}

const NO_CONTEXT: ProjectSettingsContextValue = {
  workspaceSlug: "",
  projectId: "",
  role: null,
  workspaceAdmin: false,
  project: null,
};

export const ProjectSettingsContext = React.createContext<ProjectSettingsContextValue>(NO_CONTEXT);

/** Read the shell context; tab leaves use it instead of re-reading roles. */
export function useProjectSettings(): ProjectSettingsContextValue {
  return React.useContext(ProjectSettingsContext);
}

function currentSuffix(pathname: string, workspaceSlug: string, projectId: string): string {
  const prefix = `/${workspaceSlug}/settings/projects/${projectId}`;
  if (!pathname.startsWith(prefix)) return "";
  return pathname.slice(prefix.length);
}

/**
 * Section label for the narrow top bar: the tab owning the address. The
 * first known suffix segment names it, so a tab's sub-addresses keep the
 * label while an unknown one falls back to the project name.
 */
export function currentSectionLabel(pathname: string, workspaceSlug: string, projectId: string): string | undefined {
  const suffix = currentSuffix(pathname, workspaceSlug, projectId);
  const own = findTabBySuffix(suffix);
  if (own !== undefined) return own.label;
  const segments = suffix.split("/").filter(Boolean);
  for (let length = segments.length; length > 0; length--) {
    const label = findTabBySuffix(`/${segments.slice(0, length).join("/")}`)?.label;
    if (label !== undefined) return label;
  }
  return undefined;
}

export interface ProjectSettingsShellProps {
  workspaceSlug: string;
  projectId: string;
  children: React.ReactNode;
  /**
   * Body shown inside the content column while the project read and the
   * standing reads are unresolved. Omitted means the shell renders
   * nothing until they settle; a tab that must show a loading shape of
   * its own (automations, PSET-061) opts in with its skeleton.
   */
  pendingFallback?: React.ReactNode;
}

export function ProjectSettingsShell({
  workspaceSlug,
  projectId,
  children,
  pendingFallback,
}: ProjectSettingsShellProps): React.ReactElement | null {
  const project = useProjectRow(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);
  const membership = useWorkspaceMembership(workspaceSlug);
  const join = useJoinProject();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const narrow = useNarrowViewport();
  const [drawerOpen, setDrawerOpen] = React.useState(false);

  // Standing resolves only when both role feeds answer; a failed read keeps
  // the rail hidden rather than guessing a role.
  const rolePending = roles.isPending || membership.isPending;
  const rawRole = rolePending || !roles.isSuccess ? null : (roles.data?.[projectId] ?? null);
  const workspaceRole = membership.data?.role ?? null;
  const workspaceAdmin = workspaceRole !== null && workspaceRole >= ROLE_ADMIN;
  // A workspace admin holds project-admin standing regardless of the raw
  // project row, so the rail, the role caption and every page gate agree
  // with the old pages' resolution.
  const role = effectiveProjectRole(rawRole, workspaceRole);

  // The drawer belongs to one address: every navigation closes it so a
  // deep link (or a link followed inside the drawer) starts closed.
  React.useEffect(() => {
    setDrawerOpen(false);
  }, [pathname]);

  if (project.isPending || rolePending) {
    if (pendingFallback === undefined) return null;
    return (
      <div className="flex h-full min-h-0 w-full flex-col" data-testid="project-settings-shell">
        <main aria-label="Settings content" className="min-h-0 min-w-0 flex-1 overflow-y-auto">
          {pendingFallback}
        </main>
      </div>
    );
  }

  if (!project.isSuccess) {
    const status = isApiError(project.error) ? project.error.status : undefined;
    const denial = decideProjectDenial({ status, isAdmin: workspaceAdmin });
    return (
      <ProjectSettingsRestriction
        kind={denial}
        joining={join.isPending}
        onJoin={() => join.mutate({ slug: workspaceSlug, projectId })}
      />
    );
  }

  const facts: ProjectSettingsProjectFacts = {
    name: project.data.name,
    logoProps: project.data.logo_props,
    identifier: project.data.identifier,
  };
  const sidebar = (
    <ProjectSettingsSidebar
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      projectName={facts.name}
      logoProps={facts.logoProps}
      identifier={facts.identifier}
      role={role}
      pathname={pathname}
      {...(narrow ? { className: "rounded-(--radius-dialog) border border-(--border) shadow-dialog" } : {})}
    />
  );

  return (
    <ProjectSettingsContext.Provider value={{ workspaceSlug, projectId, role, workspaceAdmin, project: facts }}>
      <div className="flex h-full min-h-0 w-full flex-col md:flex-row" data-testid="project-settings-shell">
        <div className="relative z-10 shrink-0 border-b border-(--border) md:hidden">
          <div className="flex items-center gap-(--space-2) px-(--space-3) py-(--space-2)">
            <Button
              variant="secondary"
              aria-label="Open settings navigation"
              aria-expanded={drawerOpen}
              onClick={() => setDrawerOpen((open) => !open)}
            >
              <Menu size={16} aria-hidden="true" />
            </Button>
            <span className="text-body truncate text-(--text-muted)">
              {currentSectionLabel(pathname, workspaceSlug, projectId) ?? facts.name}
            </span>
          </div>
          {drawerOpen ? (
            <>
              <button
                type="button"
                aria-label="Close settings navigation"
                data-testid="project-settings-drawer-backdrop"
                className="fixed inset-0 z-40 bg-black/20"
                onClick={() => setDrawerOpen(false)}
              />
              <div
                data-testid="project-settings-drawer"
                className="absolute top-full left-0 z-50 mt-(--space-2) max-h-[70vh]"
              >
                {sidebar}
              </div>
            </>
          ) : null}
        </div>
        <div className="hidden h-full md:block">{sidebar}</div>
        <main aria-label="Settings content" className="min-h-0 min-w-0 flex-1 overflow-y-auto">
          {children}
        </main>
      </div>
    </ProjectSettingsContext.Provider>
  );
}

export interface ProjectSettingsTabGateProps {
  tabKey: ProjectSettingsTabKey;
  children: React.ReactNode;
}

/**
 * Per-tab page gate (PSET-004). A refused role sees the not-authorized body
 * with the rail kept; General never refuses (its own screen dims for
 * non-admins). The shell provides the context only after the role
 * resolves, so a direct address never flashes the page.
 */
export function ProjectSettingsTabGate({ tabKey, children }: ProjectSettingsTabGateProps): React.ReactElement {
  const { role } = useProjectSettings();
  const tab = findTabByKey(tabKey);
  if (tab !== undefined && decidePageGate(tab, role) === "denied") {
    return <ProjectSettingsNotAuthorized heading={tab.label} />;
  }
  return <>{children}</>;
}
