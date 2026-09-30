// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Workspace sidebar (Architecture > shared/shell). Presentational: the
// workspace route loads workspaces and projects and passes them in, so
// this component never reaches past shared/ for data.

import { Avatar, Button, Kbd, Menu } from "@pidash/kit";
import { ChevronsUpDown, LogOut } from "lucide-react";
import * as React from "react";

export interface SidebarWorkspace {
  id: string;
  slug: string;
  name: string;
}

export interface SidebarProject {
  id: string;
  name: string;
  identifier: string;
}

export interface SidebarProps {
  workspaces: SidebarWorkspace[];
  activeSlug: string;
  onSelectWorkspace: (slug: string) => void;
  projects: SidebarProject[];
  activeProjectId: string | null;
  onSelectProject: (projectId: string) => void;
  userLabel: string;
  onSignOut: () => void;
}

export function Sidebar({
  workspaces,
  activeSlug,
  onSelectWorkspace,
  projects,
  activeProjectId,
  onSelectProject,
  userLabel,
  onSignOut,
}: SidebarProps): React.ReactElement {
  const active = workspaces.find((workspace) => workspace.slug === activeSlug) ?? null;
  return (
    <aside
      aria-label="Workspace navigation"
      className="flex w-60 shrink-0 flex-col gap-(--space-4) border-r border-(--border) bg-(--surface) p-(--space-4)"
    >
      <Menu
        trigger={
          <Button variant="ghost" aria-label={`Switch workspace, current: ${active?.name ?? activeSlug}`}>
            <span className="flex min-w-0 flex-1 items-center gap-(--space-2)">
              <Avatar name={active?.name ?? activeSlug} size="small" />
              <span className="text-emphasis truncate text-(--text)">{active?.name ?? activeSlug}</span>
            </span>
            <ChevronsUpDown size={16} strokeWidth={1.5} aria-hidden="true" />
          </Button>
        }
        items={workspaces.map((workspace) => ({
          label: workspace.name,
          onSelect: () => onSelectWorkspace(workspace.slug),
        }))}
      />
      <nav aria-label="Projects" className="flex min-h-0 flex-1 flex-col gap-(--space-1) overflow-auto">
        <p className="text-caption px-(--space-2) text-(--text-muted)">Projects</p>
        {projects.length === 0 ? (
          <p className="text-body px-(--space-2) text-(--text-muted)">No projects yet.</p>
        ) : (
          <ul className="flex flex-col gap-px">
            {projects.map((project) => {
              const selected = project.id === activeProjectId;
              return (
                <li key={project.id}>
                  <button
                    type="button"
                    aria-current={selected ? "page" : undefined}
                    onClick={() => onSelectProject(project.id)}
                    className={
                      selected
                        ? "text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-2) text-left text-(--text)"
                        : "text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                    }
                  >
                    <span className="text-caption shrink-0 rounded-(--radius-control) border border-(--border) px-(--space-2) py-px font-[family-name:var(--font-mono)]">
                      {project.identifier}
                    </span>
                    <span className="truncate">{project.name}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </nav>
      <div className="flex items-center gap-(--space-2) border-t border-(--border) pt-(--space-4)">
        <Avatar name={userLabel} size="small" />
        <span className="text-body min-w-0 flex-1 truncate text-(--text-muted)">{userLabel}</span>
        <Menu
          trigger={
            <Button variant="ghost" size="small" aria-label="Account actions">
              <LogOut size={16} strokeWidth={1.5} aria-hidden="true" />
            </Button>
          }
          items={[{ label: "Sign out", onSelect: onSignOut }]}
        />
      </div>
      <p className="text-caption flex items-center gap-(--space-2) px-(--space-2) text-(--text-muted)">
        Commands <Kbd keys={["mod", "K"]} />
      </p>
    </aside>
  );
}
