// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-settings section catalog (parity/specs/workspace-settings.md
// §1, WSET-001/002). The catalog is the one place that names a section's
// address, labels and minimum role. The shell filters it by the viewer's
// role and later slices attach page bodies by key through the page
// registry; neither adds sections of its own.

import { ROLE_ADMIN, ROLE_MEMBER, type Permissions } from "../../../core/session/index.js";
import { permittedWorkspaceSettingGroups } from "./perWorkspace.js";

export type WorkspaceSettingsSectionKey = "general" | "members" | "billing" | "exports" | "webhooks" | "integrations";

export type WorkspaceSettingsGroupKey = "administration" | "developer";

export interface WorkspaceSettingsSection {
  key: WorkspaceSettingsSectionKey;
  /** Visible entry and mobile crumb label. */
  label: string;
  /** Path suffix after the workspace slug; empty for general. */
  suffix: string;
  /** Tab-title section name (WSET-007). */
  title: string;
  group: WorkspaceSettingsGroupKey;
}

export interface WorkspaceSettingsGroup {
  key: WorkspaceSettingsGroupKey;
  label: string;
  sections: readonly WorkspaceSettingsSection[];
}

/** Display order of the groups that can render (Features never has entries). */
export const WORKSPACE_SETTINGS_GROUPS: readonly WorkspaceSettingsGroup[] = [
  {
    key: "administration",
    label: "Administration",
    sections: [
      {
        key: "general",
        label: "General",
        suffix: "",
        title: "General settings",
        group: "administration",
      },
      { key: "members", label: "Members", suffix: "members", title: "Members", group: "administration" },
      {
        key: "billing",
        label: "Billing & Plans",
        suffix: "billing",
        title: "Billing & Plans",
        group: "administration",
      },
      { key: "exports", label: "Exports", suffix: "exports", title: "Exports", group: "administration" },
    ],
  },
  {
    key: "developer",
    label: "Developer",
    sections: [
      { key: "webhooks", label: "Webhooks", suffix: "webhooks", title: "Webhooks", group: "developer" },
      {
        key: "integrations",
        label: "Integrations (github)",
        suffix: "integrations",
        title: "Integrations",
        group: "developer",
      },
    ],
  },
];

const SECTIONS = WORKSPACE_SETTINGS_GROUPS.flatMap((group) => group.sections);

/** Section key for the settings root (empty suffix). */
export const WORKSPACE_SETTINGS_ROOT: WorkspaceSettingsSectionKey = "general";

export function findWorkspaceSettingsSection(key: WorkspaceSettingsSectionKey): WorkspaceSettingsSection {
  const section = SECTIONS.find((entry) => entry.key === key);
  if (section === undefined) throw new Error(`Unknown workspace settings section: ${key}`);
  return section;
}

/** Workspace-relative settings path for a section, with a trailing slash. */
export function workspaceSettingsHref(workspaceSlug: string, section: WorkspaceSettingsSectionKey): string {
  const suffix = findWorkspaceSettingsSection(section).suffix;
  return `/${workspaceSlug}/settings${suffix === "" ? "" : `/${suffix}`}/`;
}

/**
 * Section a settings path belongs to, including nested detail addresses
 * (a webhook id inherits the webhooks gate, §1). Only the first segment
 * after `settings` matters, so unknown trailing segments still resolve to
 * their section. Null outside settings or for an unknown section name.
 */
export function resolveWorkspaceSettingsSection(pathname: string): WorkspaceSettingsSectionKey | null {
  const [head, settings, first] = pathname.split("/").filter(Boolean);
  if (head === undefined || settings !== "settings") return null;
  if (first === undefined) return WORKSPACE_SETTINGS_ROOT;
  return SECTIONS.find((section) => section.suffix === first)?.key ?? null;
}

export { WORKSPACE_SETTINGS_ROUTE_ID, isWorkspaceSettingsRouteId } from "./routeId.js";

/**
 * Whether the viewer may open a section: guests never; billing, webhooks
 * and integrations need admin; general, members and exports admit members.
 */
export function canOpenWorkspaceSettingsSection(
  section: WorkspaceSettingsSectionKey,
  permissions: Pick<Permissions, "canEdit" | "isAdmin">
): boolean {
  if (!permissions.canEdit) return false;
  return section === "billing" || section === "webhooks" || section === "integrations" ? permissions.isAdmin : true;
}

export interface VisibleWorkspaceSettingsSection extends WorkspaceSettingsSection {
  href: string;
}

export interface VisibleWorkspaceSettingsGroup {
  key: WorkspaceSettingsGroupKey;
  label: string;
  sections: readonly VisibleWorkspaceSettingsSection[];
}

/**
 * The sidebar groups the viewer may see, in display order. Entries and
 * whole groups without permission are absent, never disabled (§1). The
 * role filter and the per-entry addresses come from the per-workspace
 * catalog, which the account navigator's nested groups share (main#675),
 * so the two lists cannot drift.
 */
export function visibleWorkspaceSettingsGroups(
  workspaceSlug: string,
  permissions: Pick<Permissions, "canEdit" | "isAdmin">
): VisibleWorkspaceSettingsGroup[] {
  return permittedWorkspaceSettingGroups(workspaceSlug, permissions).map((group) => ({
    key: group.key,
    label: group.label,
    sections: group.links.map((link) => ({
      ...findWorkspaceSettingsSection(link.key),
      href: link.href,
    })),
  }));
}

/** Role sentence shown beside the workspace identity; empty for no role. */
export function workspaceSettingsRoleLabel(role: number | null): string {
  if (role === null) return "";
  if (role >= ROLE_ADMIN) return "Admin";
  if (role >= ROLE_MEMBER) return "Member";
  return "Guest";
}
