// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-workspace view of the settings catalog (parity/specs/
// workspace-settings.md §1; main#675). The standalone `/<ws>/settings*`
// navigator filters the catalog for the workspace in the address. The
// account-settings navigator lists every workspace the user belongs to,
// so it needs the same filtered list for a workspace other than the one
// being viewed. Both read this module, so the two lists cannot drift.

import type { Permissions } from "../../../core/session/index.js";
import {
  canOpenWorkspaceSettingsSection,
  WORKSPACE_SETTINGS_GROUPS,
  workspaceSettingsHref,
  type WorkspaceSettingsGroupKey,
  type WorkspaceSettingsSectionKey,
} from "./sections.js";

/** One permitted section of a workspace, with its address there. */
export interface WorkspaceSettingLink {
  key: WorkspaceSettingsSectionKey;
  label: string;
  href: string;
}

/** A catalog group after the role filter, or absent when nothing survives. */
export interface WorkspaceSettingGroupLinks {
  key: WorkspaceSettingsGroupKey;
  label: string;
  links: readonly WorkspaceSettingLink[];
}

/** The coarse permission set a role number stands for. */
export type RolePermissions = Pick<Permissions, "canEdit" | "isAdmin">;

/**
 * The groups and entries the viewer may open in `workspaceSlug`, in
 * catalog order, with entries and whole groups without permission left
 * out (never disabled). Entries resolve to `/<ws>/settings...` so the
 * caller can render a workspace other than the one on screen.
 */
export function permittedWorkspaceSettingGroups(
  workspaceSlug: string,
  permissions: RolePermissions
): WorkspaceSettingGroupLinks[] {
  const groups: WorkspaceSettingGroupLinks[] = [];
  for (const group of WORKSPACE_SETTINGS_GROUPS) {
    const links = group.sections
      .filter((section) => canOpenWorkspaceSettingsSection(section.key, permissions))
      .map((section) => ({
        key: section.key,
        label: section.label,
        href: workspaceSettingsHref(workspaceSlug, section.key),
      }));
    if (links.length > 0) groups.push({ key: group.key, label: group.label, links });
  }
  return groups;
}

/** One address form: both slash spellings of the same page compare equal. */
function withoutTrailingSlash(path: string): string {
  return path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
}

/**
 * Whether one entry of `workspaceSlug` reads as current. Only addresses
 * inside that workspace can match. General lives at the settings root, so
 * it needs an exact match; every other entry matches on its own subtree,
 * which keeps nested pages (a webhook id) lit under their parent entry.
 */
export function isWorkspaceSettingLinkActive(
  pathname: string,
  workspaceSlug: string,
  link: Pick<WorkspaceSettingLink, "key" | "href">
): boolean {
  const here = withoutTrailingSlash(pathname);
  const root = `/${workspaceSlug}/settings`;
  if (here !== root && !here.startsWith(`${root}/`)) return false;
  const target = withoutTrailingSlash(link.href);
  if (link.key === "general") return here === target;
  return here === target || here.startsWith(`${target}/`);
}

/** The entry that reads current, or null when none does. */
export function activeWorkspaceSettingLink<T extends WorkspaceSettingLink>(
  pathname: string,
  workspaceSlug: string,
  groups: readonly { links: readonly T[] }[]
): T | null {
  for (const group of groups) {
    for (const link of group.links) {
      if (isWorkspaceSettingLinkActive(pathname, workspaceSlug, link)) return link;
    }
  }
  return null;
}
