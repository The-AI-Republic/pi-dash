// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace sidebar (Architecture > shared/shell). Presentational: the
// workspace route loads workspaces and personal entries and passes
// them in, so this component never reaches past shared/ for data.
// The shell owns collapse, resize, peek and float; this component owns
// the content: brand, home, switcher, personal entries, the projects
// group slot and the account row. Sidebar-content slices
// (NEWFRONT-285/287) extend this content without touching the shell
// contract.

import { Avatar, Button, Kbd, Menu } from "@pidash/kit";
import { ChevronsUpDown, FileText, House, LogOut } from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../coexistence/CrossAppLink.js";
import { SidebarShell } from "./SidebarShell.js";
import { isClientSideClick, isHomeRowActive } from "./sidebarLink.js";

export interface SidebarWorkspace {
  id: string;
  slug: string;
  name: string;
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
  /** Projects group content (SHELL-048 fills this). */
  projectsGroup?: React.ReactNode;
  /** Client-side navigation for the home row's plain left clicks. */
  onNavigateHref: (href: string) => void;
}

export function Sidebar({
  workspaces,
  activeSlug,
  onSelectWorkspace,
  userLabel,
  onSignOut,
  onDeactivateAccount,
  updateControl,
  personalEntries = [],
  currentPath = null,
  onPersonalize,
  quickActions,
  projectsGroup,
  onNavigateHref,
}: SidebarProps): React.ReactElement {
  const active = workspaces.find((workspace) => workspace.slug === activeSlug) ?? null;
  const homeHref = `/${activeSlug}`;
  const homeCurrent = currentPath !== null && isHomeRowActive(currentPath, activeSlug);

  function homeClick(event: React.MouseEvent<HTMLAnchorElement>): void {
    if (!isClientSideClick(event)) return;
    event.preventDefault();
    onNavigateHref(homeHref);
  }

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
      <nav aria-label="Workspace" className="flex flex-col gap-px">
        <a
          href={homeHref}
          onClick={homeClick}
          aria-current={homeCurrent ? "page" : undefined}
          className={
            homeCurrent
              ? "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-2) text-left text-(--text)"
              : "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
          }
        >
          <House size={16} strokeWidth={1.5} aria-hidden="true" />
          <span className="truncate">Home</span>
        </a>
      </nav>
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
      {projectsGroup}
      <p className="text-caption flex items-center gap-(--space-2) px-(--space-2) text-(--text-muted)">
        Commands <Kbd keys={["mod", "K"]} />
      </p>
    </SidebarShell>
  );
}
