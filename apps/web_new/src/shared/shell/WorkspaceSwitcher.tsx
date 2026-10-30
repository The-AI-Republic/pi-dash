// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace switcher menu (SHELL-058, from parity/specs/shell.md §4).
// Presentational: the gated route passes the workspaces, the creation
// flag and every navigation callback. The active workspace lists first
// with its role, member count and check mark plus the settings and
// invite shortcuts it unlocks; the footer offers creation (unless the
// instance disables it), the invites inbox and sign-out. Mounted in
// the top bar only (NEWFRONT-1040).
import { Button, Menu, type MenuEntry } from "@pidash/kit";
import { Check, ChevronsUpDown, CirclePlus, LogOut, Mails, Settings, UserPlus } from "lucide-react";
import * as React from "react";

import { ROLE_ADMIN, ROLE_MEMBER } from "../../core/session/index.js";
import { WorkspaceLogo } from "./WorkspaceLogo.js";

export interface SwitcherWorkspace {
  id: string;
  slug: string;
  name: string;
  role: number;
  totalMembers: number;
  logoSrc: string | null;
}

export interface WorkspaceSwitcherProps {
  workspaces: SwitcherWorkspace[];
  activeSlug: string;
  /** True while the instance disables workspace creation: hides its row. */
  creationDisabled: boolean;
  onSelectWorkspace: (slug: string) => void;
  onOpenSettings: (slug: string) => void;
  onInviteMembers: (slug: string) => void;
  onCreateWorkspace: () => void;
  onWorkspaceInvites: () => void;
  onSignOut: () => void;
}

/** The active workspace first; every other row keeps list order. */
export function orderSwitcherWorkspaces(workspaces: SwitcherWorkspace[], activeSlug: string): SwitcherWorkspace[] {
  const active = workspaces.filter((workspace) => workspace.slug === activeSlug);
  const rest = workspaces.filter((workspace) => workspace.slug !== activeSlug);
  return [...active, ...rest];
}

/** The caller's workspace role as the switcher shows it; unknown reads as guest. */
export function switcherRoleLabel(role: number): string {
  if (role >= ROLE_ADMIN) return "admin";
  if (role >= ROLE_MEMBER) return "member";
  return "guest";
}

/** "1 member" or "N members"; the row counts the workspace total. */
export function formatMemberCount(totalMembers: number): string {
  return totalMembers === 1 ? "1 member" : `${totalMembers} members`;
}

function SwitcherRow({ workspace, active }: { workspace: SwitcherWorkspace; active: boolean }): React.ReactElement {
  return (
    <span className="flex min-w-0 flex-1 items-center gap-(--space-2)">
      <WorkspaceLogo src={workspace.logoSrc} name={workspace.name} />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="text-body truncate text-left text-(--text)">{workspace.name}</span>
        <span className="text-caption text-left text-(--text-muted)">
          {switcherRoleLabel(workspace.role)} · {formatMemberCount(workspace.totalMembers)}
        </span>
      </span>
      {active ? (
        <span className="inline-flex shrink-0 items-center text-(--text)">
          <Check size={16} strokeWidth={1.5} aria-hidden="true" />
          <span className="sr-only">(current workspace)</span>
        </span>
      ) : null}
    </span>
  );
}

export function WorkspaceSwitcher({
  workspaces,
  activeSlug,
  creationDisabled,
  onSelectWorkspace,
  onOpenSettings,
  onInviteMembers,
  onCreateWorkspace,
  onWorkspaceInvites,
  onSignOut,
}: WorkspaceSwitcherProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const active = workspaces.find((workspace) => workspace.slug === activeSlug) ?? null;
  const close = (): void => {
    setOpen(false);
  };

  const items: MenuEntry[] = [];
  for (const workspace of orderSwitcherWorkspaces(workspaces, activeSlug)) {
    const isActive = workspace.slug === activeSlug;
    items.push({
      label: workspace.name,
      itemKey: workspace.id,
      onSelect: () => onSelectWorkspace(workspace.slug),
      content: <SwitcherRow workspace={workspace} active={isActive} />,
    });
    // The active row unlocks its workspace shortcuts: settings for
    // members up, invites for admins. Custom buttons do not close the
    // menu on their own, so each closes it before navigating.
    if (isActive && workspace.role >= ROLE_MEMBER) {
      const extras: React.ReactNode[] = [
        <Button
          key="settings"
          variant="ghost"
          size="small"
          onClick={() => {
            close();
            onOpenSettings(workspace.slug);
          }}
        >
          <Settings size={16} strokeWidth={1.5} aria-hidden="true" />
          <span>Settings</span>
        </Button>,
      ];
      if (workspace.role >= ROLE_ADMIN) {
        extras.push(
          <Button
            key="invite"
            variant="ghost"
            size="small"
            onClick={() => {
              close();
              onInviteMembers(workspace.slug);
            }}
          >
            <UserPlus size={16} strokeWidth={1.5} aria-hidden="true" />
            <span>Invite members</span>
          </Button>
        );
      }
      items.push({
        custom: true,
        entryKey: `extras-${workspace.id}`,
        node: (
          <div data-testid="workspace-row-extras" className="flex gap-(--space-2) px-(--space-4) pb-(--space-2)">
            {extras}
          </div>
        ),
      });
    }
  }
  items.push({ separator: true });
  if (!creationDisabled) {
    items.push({
      label: "Create workspace",
      onSelect: onCreateWorkspace,
      icon: <CirclePlus size={16} strokeWidth={1.5} aria-hidden="true" />,
    });
  }
  items.push({
    label: "Workspace invites",
    onSelect: onWorkspaceInvites,
    icon: <Mails size={16} strokeWidth={1.5} aria-hidden="true" />,
  });
  items.push({
    label: "Sign out",
    onSelect: onSignOut,
    icon: <LogOut size={16} strokeWidth={1.5} aria-hidden="true" />,
  });

  return (
    <Menu
      open={open}
      onOpenChange={setOpen}
      trigger={
        <Button
          variant="ghost"
          aria-label="Open workspace switcher"
          data-testid="topbar-workspace-switcher"
          data-keep-open="true"
        >
          <span className="flex min-w-0 flex-1 items-center gap-(--space-2)">
            <WorkspaceLogo src={active?.logoSrc ?? null} name={active?.name ?? activeSlug} />
            <span className="text-emphasis truncate text-(--text)">{active?.name ?? activeSlug}</span>
          </span>
          <ChevronsUpDown size={16} strokeWidth={1.5} aria-hidden="true" />
        </Button>
      }
      items={items}
    />
  );
}
