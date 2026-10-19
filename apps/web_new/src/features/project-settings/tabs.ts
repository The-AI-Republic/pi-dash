// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-settings tab registry (parity/specs/project-settings-shell.md,
// PSET-001–004). Pure: the grouped, role-filtered navigation model, the
// suffix → tab resolution shared by the sidebar, the mobile bar and every
// leaf route, and the per-tab permission answer. Nothing here reads React,
// the router or the network.

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../core/session/permissions.js";

/** The tab groups in display order (PSET-001). */
export const PROJECT_SETTINGS_GROUPS = ["general", "features", "work-structure", "execution"] as const;
export type ProjectSettingsGroup = (typeof PROJECT_SETTINGS_GROUPS)[number];

export const PROJECT_SETTINGS_GROUP_LABELS: Record<ProjectSettingsGroup, string> = {
  general: "General",
  features: "Features",
  "work-structure": "Work structure",
  execution: "Execution",
};

export type ProjectSettingsTabKey =
  | "general"
  | "members"
  | "cycles"
  | "modules"
  | "views"
  | "pages"
  | "intake"
  | "states"
  | "labels"
  | "estimates"
  | "automations"
  | "github"
  | "schedulers";

/** Roles a project tab is offered to, by the sidebar registry (PSET-004). */
export type ProjectSettingsStanding = "everyone" | "member" | "admin";

export interface ProjectSettingsTabDef {
  key: ProjectSettingsTabKey;
  /** Navigation label. Repository stands in for the github suffix. */
  label: string;
  /** Address suffix under the settings root; "" is General. */
  suffix: string;
  group: ProjectSettingsGroup;
  /** Sidebar visibility standing. */
  visibleTo: ProjectSettingsStanding;
}

/**
 * Every tab in sidebar order (PSET-001). Groups render General, Features,
 * Work structure, Execution; each keeps its registered order.
 */
export const PROJECT_SETTINGS_TABS: readonly ProjectSettingsTabDef[] = [
  { key: "general", label: "General", suffix: "", group: "general", visibleTo: "everyone" },
  { key: "members", label: "Members", suffix: "/members", group: "general", visibleTo: "everyone" },
  { key: "cycles", label: "Cycles", suffix: "/features/cycles", group: "features", visibleTo: "admin" },
  { key: "modules", label: "Modules", suffix: "/features/modules", group: "features", visibleTo: "admin" },
  { key: "views", label: "Views", suffix: "/features/views", group: "features", visibleTo: "admin" },
  { key: "pages", label: "Pages", suffix: "/features/pages", group: "features", visibleTo: "admin" },
  { key: "intake", label: "Intake", suffix: "/features/intake", group: "features", visibleTo: "admin" },
  { key: "states", label: "States", suffix: "/states", group: "work-structure", visibleTo: "member" },
  { key: "labels", label: "Labels", suffix: "/labels", group: "work-structure", visibleTo: "member" },
  { key: "estimates", label: "Estimates", suffix: "/estimates", group: "work-structure", visibleTo: "admin" },
  { key: "automations", label: "Automations", suffix: "/automations", group: "execution", visibleTo: "admin" },
  { key: "github", label: "Repository", suffix: "/github", group: "execution", visibleTo: "admin" },
  { key: "schedulers", label: "Schedulers", suffix: "/schedulers", group: "execution", visibleTo: "admin" },
];

/** A project role number, or null when the viewer holds no membership. */
export type ProjectRole = number | null;

/** True for admin or member standing; guests and outsiders read false. */
export function isMemberStanding(role: ProjectRole): boolean {
  return role !== null && role >= ROLE_MEMBER;
}

/** True for admin standing only. */
export function isAdminStanding(role: ProjectRole): boolean {
  return role !== null && role >= ROLE_ADMIN;
}

/** Whether the standing admits an entry registered for `visibleTo`. */
export function admitsStanding(role: ProjectRole, visibleTo: ProjectSettingsStanding): boolean {
  if (visibleTo === "everyone") return role !== null && role >= ROLE_GUEST;
  if (visibleTo === "member") return isMemberStanding(role);
  return isAdminStanding(role);
}

export interface ProjectSettingsNavGroup {
  key: ProjectSettingsGroup;
  label: string;
  tabs: ProjectSettingsTabDef[];
}

/**
 * The grouped sidebar for one viewer role (PSET-001/PSET-004). Entries the
 * role cannot use are absent, not disabled, and a group with no surviving
 * entry disappears entirely.
 */
export function selectSettingsNav(role: ProjectRole): ProjectSettingsNavGroup[] {
  const groups: ProjectSettingsNavGroup[] = [];
  for (const key of PROJECT_SETTINGS_GROUPS) {
    const tabs = PROJECT_SETTINGS_TABS.filter((tab) => tab.group === key && admitsStanding(role, tab.visibleTo));
    if (tabs.length > 0) groups.push({ key, label: PROJECT_SETTINGS_GROUP_LABELS[key], tabs });
  }
  return groups;
}

/** Root address of the per-project settings area, without a trailing slash. */
export function projectSettingsRoot(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/settings/projects/${projectId}`;
}

/**
 * Canonical slash-terminated address for one tab, the form every sidebar
 * entry links to (PSET-003).
 */
export function projectSettingsTabHref(workspaceSlug: string, projectId: string, tab: ProjectSettingsTabDef): string {
  return `${projectSettingsRoot(workspaceSlug, projectId)}${tab.suffix}/`;
}

/** Tab for a registry key, or undefined for an unknown one. */
export function findTabByKey(key: ProjectSettingsTabKey): ProjectSettingsTabDef | undefined {
  return PROJECT_SETTINGS_TABS.find((tab) => tab.key === key);
}

/** Tab for an address suffix, or undefined for an unknown one. */
export function findTabBySuffix(suffix: string): ProjectSettingsTabDef | undefined {
  const normalized = normalizeSuffix(suffix);
  return PROJECT_SETTINGS_TABS.find((tab) => tab.suffix === normalized);
}

function normalizeSuffix(suffix: string): string {
  if (suffix === "" || suffix === "/") return "";
  return suffix.endsWith("/") ? suffix.slice(0, -1) : suffix;
}

/**
 * Resolve the settings suffix of a pathname below the workspace root.
 * Returns the tab when the address names one, or undefined when it names
 * no known tab. Both slash forms resolve to the same tab, so a typed
 * address and a sidebar link render identical content (PSET-003; the
 * highlight is a separate, slash-sensitive marker — NEWFRONT-396).
 */
export function resolveSettingsAddress(
  pathname: string,
  workspaceSlug: string
): { projectId: string; tab: ProjectSettingsTabDef } | undefined {
  const prefix = `/${workspaceSlug}/settings/projects/`;
  const rest = pathname.startsWith(prefix) ? pathname.slice(prefix.length) : null;
  if (rest === null || rest === "") return undefined;
  const segments = rest.split("/");
  const projectId = segments[0] ?? "";
  if (projectId === "") return undefined;
  const suffix = segments.length === 1 ? "" : `/${segments.slice(1).filter(Boolean).join("/")}`;
  const tab = findTabBySuffix(suffix);
  if (tab === undefined) return undefined;
  return { projectId, tab };
}

/**
 * Address patterns on which a tab carries the "selected" marker (PSET-001,
 * PSET-062): the canonical slash form only. The no-slash form renders the
 * same content and marks nothing, pinned as bug NEWFRONT-396 (same for the
 * root, where General reads `.../{id}/`). Addresses below the tab's own
 * address are not part of any tab.
 */
export function isTabActive(
  pathname: string,
  workspaceSlug: string,
  projectId: string,
  tab: ProjectSettingsTabDef
): boolean {
  return pathname === `${projectSettingsRoot(workspaceSlug, projectId)}${tab.suffix}/`;
}

/** Sidebar entry for a role, or undefined when the sidebar hides it. */
export function findVisibleTab(key: ProjectSettingsTabKey, role: ProjectRole): ProjectSettingsTabDef | undefined {
  const tab = PROJECT_SETTINGS_TABS.find((entry) => entry.key === key);
  if (tab === undefined) return undefined;
  return admitsStanding(role, tab.visibleTo) ? tab : undefined;
}

/**
 * Which page screen a tab renders for a role: the tab itself, the
 * not-authorized body, or the general form. Page gates follow the sidebar
 * except for the two pinned mismatches (PSET-004): the Members page is
 * stricter than its sidebar entry (guest refused, sidebar kept), and the
 * Repository page is wider (members admitted though the sidebar hides it).
 */
export type ProjectSettingsPageGate = "allowed" | "denied";

/**
 * Page gate for one tab, driven by the project role alone (PSET-004; the
 * oracle pins the Members page refusing a project guest even when that
 * person administers the workspace). Only General never refuses; the
 * Members page is stricter than its sidebar entry, and Repository wider.
 */
export function decidePageGate(tab: ProjectSettingsTabDef, role: ProjectRole): ProjectSettingsPageGate {
  if (role === null || role < ROLE_GUEST) return "denied";
  switch (tab.key) {
    case "general":
      return "allowed";
    case "members":
      return isMemberStanding(role) ? "allowed" : "denied";
    case "states":
    case "labels":
      return isMemberStanding(role) ? "allowed" : "denied";
    case "github":
      return isMemberStanding(role) ? "allowed" : "denied";
    default:
      return isAdminStanding(role) ? "allowed" : "denied";
  }
}
