// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// User menu (SHELL-059 target, from parity/specs/shell.md §11): a plain
// avatar-plus-name-plus-email block with no cover art, a Your-work
// entry for admins and members, a settings-page entry and sign-out.
// Presentational: the gated route passes the identity and every
// navigation callback. The sidebar variant triggers off the full
// account card; the compact variant triggers off the avatar alone in
// the top bar. Neither variant touches sidebar peek state.
import { Avatar, Button, Menu } from "@pidash/kit";
import { Briefcase, ChevronsUpDown, LogOut, Settings } from "lucide-react";
import * as React from "react";

import { ROLE_MEMBER } from "../../core/session/index.js";

export interface UserMenuProps {
  displayName: string;
  email: string;
  /** Resolved avatar URL; null renders initials. */
  avatarSrc: string | null;
  /** The caller's role in the active workspace; gates Your work. */
  role: number;
  /** Sidebar card trigger or compact top-bar avatar trigger. */
  variant: "sidebar" | "compact";
  onYourWork: () => void;
  onSettings: () => void;
  /**
   * Opens the deactivate-account dialog. Interim placement (AUTH-019):
   * the settings page relocates this trigger when it lands. Absent,
   * the menu offers sign-out only after settings.
   */
  onDeactivateAccount?: (() => void) | undefined;
  onSignOut: () => void;
}

export function UserMenu({
  displayName,
  email,
  avatarSrc,
  role,
  variant,
  onYourWork,
  onSettings,
  onDeactivateAccount,
  onSignOut,
}: UserMenuProps): React.ReactElement {
  const trigger =
    variant === "sidebar" ? (
      <Button variant="ghost" className="w-full min-w-0" data-testid="sidebar-user-menu" data-keep-open="true">
        <Avatar name={displayName} src={avatarSrc ?? ""} size="small" />
        <span className="flex min-w-0 flex-1 flex-col text-left">
          <span className="text-body truncate text-(--text)">{displayName}</span>
          <span className="text-caption truncate text-(--text-muted)">{email}</span>
        </span>
        <ChevronsUpDown size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
      </Button>
    ) : (
      <Button variant="ghost" size="small" aria-label="Open user menu" data-testid="topbar-user-menu">
        <Avatar name={displayName} src={avatarSrc ?? ""} size="small" />
      </Button>
    );
  return (
    <Menu
      trigger={trigger}
      items={[
        {
          custom: true,
          entryKey: "identity",
          node: (
            <div
              data-testid="user-menu-identity"
              className="flex items-center gap-(--space-3) px-(--space-4) py-(--space-3)"
            >
              <Avatar name={displayName} src={avatarSrc ?? ""} size="medium" />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="text-body truncate text-(--text)">{displayName}</span>
                <span className="text-caption truncate text-(--text-muted)">{email}</span>
              </span>
            </div>
          ),
        },
        { separator: true },
        ...(role >= ROLE_MEMBER
          ? [
              {
                label: "Your work",
                onSelect: onYourWork,
                icon: <Briefcase size={16} strokeWidth={1.5} aria-hidden="true" />,
              },
            ]
          : []),
        {
          label: "Settings",
          onSelect: onSettings,
          icon: <Settings size={16} strokeWidth={1.5} aria-hidden="true" />,
        },
        ...(onDeactivateAccount === undefined ? [] : [{ label: "Deactivate account", onSelect: onDeactivateAccount }]),
        {
          label: "Sign out",
          onSelect: onSignOut,
          icon: <LogOut size={16} strokeWidth={1.5} aria-hidden="true" />,
        },
      ]}
    />
  );
}
