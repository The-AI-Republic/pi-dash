// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project card menu (SHELL-034). One entry list feeds both the header
// quick-actions menu and the right-click menu: settings for project
// admins and members, join plus open-in-tab for outsiders, restore and
// delete for project admins on archived cards, copy-link for everyone
// on active cards. Labels are the parity contract the gate asserts.
import type { MenuEntry } from "@pidash/kit";
import { ArchiveRestore, ExternalLink, Link2, Settings, Trash2, UserPlus } from "lucide-react";
import * as React from "react";

export interface CardMenuStanding {
  /** Archived cards never navigate; only restore/delete apply. */
  archived: boolean;
  /** Any project membership (admin, member or guest). */
  isMember: boolean;
  /** Admin or member standing: settings, no join prompt. */
  isAuthorized: boolean;
  /** Project admin: restore and delete on archived cards. */
  isAdmin: boolean;
}

export interface CardMenuCallbacks {
  onSettings: () => void;
  onJoin: () => void;
  onOpenInNewTab: () => void;
  onCopyLink: () => void;
  /** Null until NEWFRONT-284 wires the dialog; the entry hides. */
  onRestore: (() => void) | null;
  /** Null until NEWFRONT-284 wires the dialog; the entry hides. */
  onDelete: (() => void) | null;
}

/** Entries the standing permits, in menu order. */
export function selectCardMenuEntries(standing: CardMenuStanding, callbacks: CardMenuCallbacks): MenuEntry[] {
  const entries: MenuEntry[] = [];
  if (!standing.archived && standing.isAuthorized) {
    entries.push({
      label: "Settings",
      icon: <Settings size={16} strokeWidth={1.5} aria-hidden="true" />,
      onSelect: callbacks.onSettings,
    });
  }
  if (!standing.archived && !standing.isMember) {
    entries.push({
      label: "Join",
      icon: <UserPlus size={16} strokeWidth={1.5} aria-hidden="true" />,
      onSelect: callbacks.onJoin,
    });
    entries.push({
      label: "Open in new tab",
      icon: <ExternalLink size={16} strokeWidth={1.5} aria-hidden="true" />,
      onSelect: callbacks.onOpenInNewTab,
    });
  }
  if (!standing.archived) {
    entries.push({
      label: "Copy link",
      icon: <Link2 size={16} strokeWidth={1.5} aria-hidden="true" />,
      onSelect: callbacks.onCopyLink,
    });
  }
  if (standing.archived && standing.isAdmin && callbacks.onRestore !== null) {
    entries.push({
      label: "Restore",
      icon: <ArchiveRestore size={16} strokeWidth={1.5} aria-hidden="true" />,
      onSelect: callbacks.onRestore,
    });
  }
  if (standing.archived && standing.isAdmin && callbacks.onDelete !== null) {
    entries.push({
      label: "Delete",
      icon: <Trash2 size={16} strokeWidth={1.5} aria-hidden="true" />,
      onSelect: callbacks.onDelete,
    });
  }
  return entries;
}

/** Work-items address of a project, for links, copies and new tabs. */
export function projectIssuesPath(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/issues`;
}

/** Project settings address, served cross-app until settings migrates. */
export function projectSettingsPath(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/settings/projects/${projectId}`;
}
