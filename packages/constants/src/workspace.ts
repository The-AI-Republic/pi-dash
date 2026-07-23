/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TStaticViewTypes, IWorkspaceSearchResults } from "@pi-dash/types";
import { EUserWorkspaceRoles } from "@pi-dash/types";

export const ORGANIZATION_SIZE: string[] = ["Just myself", "2-10", "11-50", "51-200", "201-500", "500+"];

export const RESTRICTED_URLS: string[] = [
  "404",
  "accounts",
  "api",
  "create-workspace",
  "god-mode",
  "installations",
  "invitations",
  "onboarding",
  "profile",
  "spaces",
  "workspace-invitations",
  "password",
  "flags",
  "monitor",
  "monitoring",
  "ingest",
  "pi-dash-pro",
  "pi-dash-ultimate",
  "enterprise",
  "pi-dash-enterprise",
  "disco",
  "silo",
  "chat",
  "calendar",
  "drive",
  "channels",
  "upgrade",
  "billing",
  "sign-in",
  "sign-up",
  "signin",
  "signup",
  "config",
  "live",
  "admin",
  "m",
  "import",
  "importers",
  "integrations",
  "integration",
  "configuration",
  "initiatives",
  "initiative",
  "config",
  "workflow",
  "workflows",
  "epics",
  "epic",
  "story",
  "mobile",
  "dashboard",
  "desktop",
  "onload",
  "real-time",
  "one",
  "pages",
  "mobile",
  "business",
  "pro",
  "settings",
  "monitor",
  "license",
  "licenses",
  "instances",
  "instance",
];

export const ROLE = {
  [EUserWorkspaceRoles.GUEST]: "Guest",
  [EUserWorkspaceRoles.MEMBER]: "Member",
  [EUserWorkspaceRoles.ADMIN]: "Admin",
};

export const ROLE_DETAILS = {
  [EUserWorkspaceRoles.GUEST]: {
    i18n_title: "Guest",
    i18n_description: "External members of organizations can be invited as guests.",
  },
  [EUserWorkspaceRoles.MEMBER]: {
    i18n_title: "Member",
    i18n_description: "Ability to read, write, edit, and delete entities inside projects, cycles, and modules",
  },
  [EUserWorkspaceRoles.ADMIN]: {
    i18n_title: "Admin",
    i18n_description: "All permissions set to true within the workspace.",
  },
};

export const USER_ROLES = [
  {
    value: "Product / Project Manager",
    i18n_label: "Product / Project Manager",
  },
  {
    value: "Development / Engineering",
    i18n_label: "Development / Engineering",
  },
  {
    value: "Founder / Executive",
    i18n_label: "Founder / Executive",
  },
  {
    value: "Freelancer / Consultant",
    i18n_label: "Freelancer / Consultant",
  },
  { value: "Marketing / Growth", i18n_label: "Marketing / Growth" },
  {
    value: "Sales / Business Development",
    i18n_label: "Sales / Business Development",
  },
  {
    value: "Support / Operations",
    i18n_label: "Support / Operations",
  },
  {
    value: "Student / Professor",
    i18n_label: "Student / Professor",
  },
  { value: "Human Resources", i18n_label: "Human / Resources" },
  { value: "Other", i18n_label: "Other" },
];

export const IMPORTERS_LIST = [
  {
    provider: "github",
    type: "import",
    i18n_title: "Github",
    i18n_description: "Import work items from GitHub repositories and sync them.",
  },
  {
    provider: "jira",
    type: "import",
    i18n_title: "Jira",
    i18n_description: "Import work items and epics from Jira projects and epics.",
  },
];

export const EXPORTERS_LIST = [
  {
    provider: "csv",
    type: "export",
    i18n_title: "CSV",
    i18n_description: "Export work items to a CSV file.",
  },
  {
    provider: "xlsx",
    type: "export",
    i18n_title: "Excel",
    i18n_description: "Export work items to a CSV file.",
  },
  {
    provider: "json",
    type: "export",
    i18n_title: "JSON",
    i18n_description: "Export work items to a CSV file.",
  },
];

export const DEFAULT_GLOBAL_VIEWS_LIST: {
  key: TStaticViewTypes;
  i18n_label: string;
}[] = [
  {
    key: "all-issues",
    i18n_label: "All work items",
  },
  {
    key: "assigned",
    i18n_label: "Assigned",
  },
  {
    key: "created",
    i18n_label: "Created",
  },
  {
    key: "subscribed",
    i18n_label: "Subscribed",
  },
];

export interface IWorkspaceSidebarNavigationItem {
  key: string;
  labelTranslationKey: string;
  href: string;
  access: EUserWorkspaceRoles[];
  highlight: (pathname: string, url: string) => boolean;
  tooltipTranslationKey?: string;
}

export const WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS: Record<string, IWorkspaceSidebarNavigationItem> = {
  views: {
    key: "views",
    labelTranslationKey: "Views",
    href: `/all-issues/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER, EUserWorkspaceRoles.GUEST],
    highlight: (pathname: string, url: string) => pathname.includes(url),
  },
  analytics: {
    key: "analytics",
    labelTranslationKey: "Analytics",
    href: `/analytics/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
    highlight: (pathname: string, url: string) => pathname.includes(url),
  },
  archives: {
    key: "archives",
    labelTranslationKey: "Archives",
    href: `/projects/archives/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
    highlight: (pathname: string, url: string) => pathname.includes(url),
  },
};

export const WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS_LINKS: IWorkspaceSidebarNavigationItem[] = [
  WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS["views"],
  WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS["analytics"],
  WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS["archives"],
];

export const WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS: Record<string, IWorkspaceSidebarNavigationItem> = {
  home: {
    key: "home",
    labelTranslationKey: "Home",
    href: `/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER, EUserWorkspaceRoles.GUEST],
    highlight: (pathname: string, url: string) => pathname === url,
  },
  assistant: {
    key: "assistant",
    labelTranslationKey: "Pi Dash AI",
    href: `/assistant/`,
    // Guests are excluded (parity with the backend 403).
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
    highlight: (pathname: string, url: string) => pathname.includes(url),
  },
  inbox: {
    key: "inbox",
    labelTranslationKey: "Inbox",
    href: `/notifications/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER, EUserWorkspaceRoles.GUEST],
    highlight: (pathname: string, url: string) => pathname.includes(url),
  },
  "your-work": {
    key: "your_work",
    labelTranslationKey: "Your work",
    href: `/profile/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
    highlight: (pathname: string, url: string) => pathname.includes(url),
  },
  stickies: {
    key: "stickies",
    labelTranslationKey: "Stickies",
    href: `/stickies/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER, EUserWorkspaceRoles.GUEST],
    highlight: (pathname: string, url: string) => pathname.includes(url),
  },
  drafts: {
    key: "drafts",
    labelTranslationKey: "Drafts",
    href: `/drafts/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
    highlight: (pathname: string, url: string) => pathname.includes(url),
  },
  projects: {
    key: "projects",
    labelTranslationKey: "Projects",
    href: `/projects/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER, EUserWorkspaceRoles.GUEST],
    highlight: (pathname: string, url: string) => pathname === url,
    tooltipTranslationKey: "Browse projects",
  },
  runners: {
    key: "runners",
    labelTranslationKey: "AI Workers",
    href: `/runners/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
    highlight: (pathname: string, url: string) => pathname.includes(url),
    tooltipTranslationKey: "Manage your AI Worker connectivities",
  },
  prompts: {
    key: "prompts",
    labelTranslationKey: "Prompts",
    href: `/prompts/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER, EUserWorkspaceRoles.GUEST],
    highlight: (pathname: string, url: string) => pathname.includes(url),
    tooltipTranslationKey: "AI Prompt Templates",
  },
  // Project Scheduler — sibling of Prompts (not nested). Visible to all
  // workspace members; the underlying API gates mutation per role.
  // See .ai_design/project_scheduler/design.md §8.A.
  schedulers: {
    key: "schedulers",
    labelTranslationKey: "Schedulers",
    href: `/schedulers/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER, EUserWorkspaceRoles.GUEST],
    highlight: (pathname: string, url: string) => pathname.includes(url),
    tooltipTranslationKey: "Recurring AI Agent jobs scoped to projects",
  },
  "ai-dev-machines": {
    key: "ai_dev_machines",
    labelTranslationKey: "AI Dev Machines",
    href: `/ai-dev-machines/`,
    access: [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
    highlight: (pathname: string, url: string) => pathname.includes(url),
    tooltipTranslationKey: "Install the pidash CLI and register dev machines as AI agent runners",
  },
};

export const WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS_LINKS: IWorkspaceSidebarNavigationItem[] = [
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["home"],
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["assistant"],
];

export const WORKSPACE_SIDEBAR_STATIC_PINNED_NAVIGATION_ITEMS_LINKS: IWorkspaceSidebarNavigationItem[] = [
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["projects"],
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["runners"],
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["prompts"],
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS["schedulers"],
];

export const IS_FAVORITE_MENU_OPEN = "is_favorite_menu_open";
export const WORKSPACE_DEFAULT_SEARCH_RESULT: IWorkspaceSearchResults = {
  results: {
    workspace: [],
    project: [],
    issue: [],
    cycle: [],
    module: [],
    issue_view: [],
    page: [],
  },
};

export const USE_CASES = [
  "Plan and track product roadmaps",
  "Manage engineering sprints",
  "Coordinate cross-functional projects",
  "Replace our current tool",
  "Just exploring",
];
