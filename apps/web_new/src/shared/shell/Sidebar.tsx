// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace sidebar (Architecture > shared/shell). Presentational: the
// workspace route loads workspaces, projects and personal entries and
// passes them in, so this component never reaches past shared/ for data.
// The shell owns collapse, resize, peek and float; this component owns
// the content: brand, switcher, personal entries, projects and the
// account row. Sidebar-content slices (NEWFRONT-285/287) extend this
// content without touching the shell contract.

import { Avatar, Button, Kbd, Menu } from "@pidash/kit";
import { ChevronsUpDown, FileText, LogOut } from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../coexistence/CrossAppLink.js";
import { SidebarShell } from "./SidebarShell.js";

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

/**
 * One personal navigation entry (SHELL-070). The route derives the
 * visible, ordered list from sidebar preferences; the dialog edits them.
 */
export interface SidebarPersonalEntry {
  key: string;
  label: string;
  href: string;
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
  /**
   * Opens the deactivate-account dialog. Interim placement (AUTH-019): the
   * settings page (NEWFRONT-90) relocates this trigger when it lands.
   * Absent, the menu offers sign-out only.
   */
  onDeactivateAccount?: () => void;
  /**
   * Desktop update control mount (SHELL-102, from parity/specs/shell.md
   * §5): the Desktop area renders its waiting-update button here; the
   * shell owns only the mount point beside the account area.
   */
  updateControl?: React.ReactNode;
  /** Personal entries above projects, already filtered and ordered. */
  personalEntries?: SidebarPersonalEntry[];
  /** Current pathname; entries matching it read as current. */
  currentPath?: string | null;
  /**
   * Opens the personalization dialog (SHELL-070/071). Without it the
   * header launcher stays unmounted.
   */
  onPersonalize?: () => void;
  /** Quick actions under the header (SHELL-047 fills this). */
  quickActions?: React.ReactNode;
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
  onDeactivateAccount,
  updateControl,
  personalEntries = [],
  currentPath = null,
  onPersonalize,
  quickActions,
}: SidebarProps): React.ReactElement {
  const active = workspaces.find((workspace) => workspace.slug === activeSlug) ?? null;
  return (
    <SidebarShell
      brand={
        <span data-testid="sidebar-brand" className="text-emphasis truncate text-(--text)">
          Pi Dash
        </span>
      }
      onPersonalize={onPersonalize}
      quickActions={quickActions}
      account={
        <>
          <Avatar name={userLabel} size="small" />
          <span className="text-body min-w-0 flex-1 truncate text-(--text-muted)">{userLabel}</span>
          {updateControl}
          <Menu
            trigger={
              <Button variant="ghost" size="small" aria-label="Account actions" data-testid="sidebar-account-menu">
                <LogOut size={16} strokeWidth={1.5} aria-hidden="true" />
              </Button>
            }
            items={[
              { label: "Sign out", onSelect: onSignOut },
              ...(onDeactivateAccount === undefined
                ? []
                : [{ label: "Deactivate account", onSelect: onDeactivateAccount }]),
            ]}
          />
        </>
      }
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
      {personalEntries.length === 0 ? null : (
        <nav aria-label="Personal" className="flex flex-col gap-(--space-1)">
          <ul className="flex flex-col gap-px">
            {personalEntries.map((entry) => {
              const current = currentPath?.startsWith(entry.href) ?? false;
              return (
                <li key={entry.key}>
                  <CrossAppLink
                    to={entry.href}
                    aria-current={current ? "page" : undefined}
                    className={
                      current
                        ? "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-2) text-left text-(--text)"
                        : "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                    }
                  >
                    <FileText size={16} strokeWidth={1.5} aria-hidden="true" />
                    <span className="truncate">{entry.label}</span>
                  </CrossAppLink>
                </li>
              );
            })}
          </ul>
        </nav>
      )}
      <nav aria-label="Projects" className="flex flex-col gap-(--space-1)">
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
      <p className="text-caption flex items-center gap-(--space-2) px-(--space-2) text-(--text-muted)">
        Commands <Kbd keys={["mod", "K"]} />
      </p>
    </SidebarShell>
  );
}
