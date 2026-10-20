// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace sidebar (Architecture > shared/shell). Presentational: the
// workspace route loads workspaces and personal entries and passes
// them in, so this component never reaches past shared/ for data.
// The shell owns collapse, resize, peek and float; this component owns
// the content: brand, home, switcher, personal entries, the projects
// group slot and the account row. Sidebar-content slices
// (NEWFRONT-285/286/287) extend this content without touching the
// shell contract.

import { Kbd } from "@pidash/kit";
import { Blocks, FileText, House } from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../coexistence/CrossAppLink.js";
import { SidebarShell } from "./SidebarShell.js";
import { isClientSideClick, isHomeRowActive } from "./sidebarLink.js";
import { UserMenu } from "./UserMenu.js";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher.js";

export interface SidebarWorkspace {
  id: string;
  slug: string;
  name: string;
  role: number;
  totalMembers: number;
  /** Resolved logo URL; null renders the workspace initial. */
  logoSrc: string | null;
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

/**
 * One edition-contributed navigation entry (CLOUD-021). The workspace
 * route resolves these from `edition.sidebar`; the OSS edition
 * contributes none, so its sidebar renders exactly as before.
 */
export interface SidebarEditionEntry {
  key: string;
  label: string;
  href: string;
}

export interface SidebarProps {
  workspaces: SidebarWorkspace[];
  activeSlug: string;
  onSelectWorkspace: (slug: string) => void;
  userLabel: string;
  /** Signed-in identity details for the user menu. */
  userEmail: string;
  /** Resolved avatar URL; null renders initials. */
  userAvatarSrc: string | null;
  /** The caller's role in the active workspace; gates Your work. */
  userRole: number;
  /** Opens the current user's profile (Your work). */
  onYourWork: () => void;
  /** Opens the settings page. */
  onUserSettings: () => void;
  /** True while the instance disables workspace creation: hides its row. */
  creationDisabled: boolean;
  /** Opens one workspace's settings page. */
  onOpenSettings: (slug: string) => void;
  /** Opens one workspace's member invites. */
  onInviteMembers: (slug: string) => void;
  /** Opens workspace creation. */
  onCreateWorkspace: () => void;
  /** Opens the workspace invites inbox. */
  onWorkspaceInvites: () => void;
  onSignOut: () => void;
  /**
   * Opens the deactivate-account dialog. Interim placement (AUTH-019): the
   * settings page (NEWFRONT-90) relocates this trigger when it lands.
   * Absent, the menu offers sign-out only after settings.
   */
  onDeactivateAccount?: () => void;
  /**
   * Desktop update control mount (SHELL-102, from parity/specs/shell.md
   * §5): the Desktop area renders its waiting-update button here; the
   * shell owns only the mount point beside the account area.
   */
  updateControl?: React.ReactNode;
  /**
   * Workspace plan badge (CLOUD-038). The gated route mounts it only on
   * editions that publish a plan snapshot; it sits above the account row,
   * where the old app kept its edition entry.
   */
  planBadge?: React.ReactNode;
  /** Personal entries above projects, already filtered and ordered. */
  personalEntries?: SidebarPersonalEntry[];
  /**
   * Edition entries beside Home (CLOUD-021). Rendered for every role —
   * installations belong to the viewer, so no role is excluded — and
   * absent entirely when the edition contributes none.
   */
  editionEntries?: SidebarEditionEntry[];
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
  /** Secondary-destinations disclosure (SHELL-053 fills this). */
  moreGroup?: React.ReactNode;
  /** Favorites section content (SHELL-056 fills this). */
  favoritesGroup?: React.ReactNode;
  /** Client-side navigation for the home row's plain left clicks. */
  onNavigateHref: (href: string) => void;
}

export function Sidebar({
  workspaces,
  activeSlug,
  onSelectWorkspace,
  userLabel,
  userEmail,
  userAvatarSrc,
  userRole,
  onYourWork,
  onUserSettings,
  creationDisabled,
  onOpenSettings,
  onInviteMembers,
  onCreateWorkspace,
  onWorkspaceInvites,
  onSignOut,
  onDeactivateAccount,
  updateControl,
  planBadge,
  personalEntries = [],
  editionEntries = [],
  currentPath = null,
  onPersonalize,
  quickActions,
  projectsGroup,
  moreGroup,
  favoritesGroup,
  onNavigateHref,
}: SidebarProps): React.ReactElement {
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
          {planBadge === undefined ? null : <div data-testid="sidebar-plan-badge">{planBadge}</div>}
          <div className="min-w-0 flex-1">
            <UserMenu
              displayName={userLabel}
              email={userEmail}
              avatarSrc={userAvatarSrc}
              role={userRole}
              variant="sidebar"
              onYourWork={onYourWork}
              onSettings={onUserSettings}
              onDeactivateAccount={onDeactivateAccount}
              onSignOut={onSignOut}
            />
          </div>
          {updateControl}
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
        {editionEntries.map((entry) => {
          const current = currentPath?.startsWith(entry.href) ?? false;
          return (
            <CrossAppLink
              key={entry.key}
              to={entry.href}
              aria-current={current ? "page" : undefined}
              className={
                current
                  ? "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-2) text-left text-(--text)"
                  : "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
              }
            >
              <Blocks size={16} strokeWidth={1.5} aria-hidden="true" />
              <span className="truncate">{entry.label}</span>
            </CrossAppLink>
          );
        })}
      </nav>
      <WorkspaceSwitcher
        workspaces={workspaces}
        activeSlug={activeSlug}
        creationDisabled={creationDisabled}
        onSelectWorkspace={onSelectWorkspace}
        onOpenSettings={onOpenSettings}
        onInviteMembers={onInviteMembers}
        onCreateWorkspace={onCreateWorkspace}
        onWorkspaceInvites={onWorkspaceInvites}
        onSignOut={onSignOut}
        mount="sidebar"
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
      {moreGroup}
      {favoritesGroup}
      <p className="text-caption flex items-center gap-(--space-2) px-(--space-2) text-(--text-muted)">
        Commands <Kbd keys={["mod", "K"]} />
      </p>
    </SidebarShell>
  );
}
