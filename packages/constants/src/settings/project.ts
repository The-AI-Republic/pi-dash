/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash imports
import { EUserProjectRoles } from "@pi-dash/types";
import type { TProjectSettingsItem, TProjectSettingsTabs } from "@pi-dash/types";

export enum PROJECT_SETTINGS_CATEGORY {
  GENERAL = "general",
  FEATURES = "features",
  WORK_STRUCTURE = "work-structure",
  EXECUTION = "execution",
}

export const PROJECT_SETTINGS_CATEGORIES: PROJECT_SETTINGS_CATEGORY[] = [
  PROJECT_SETTINGS_CATEGORY.GENERAL,
  PROJECT_SETTINGS_CATEGORY.FEATURES,
  PROJECT_SETTINGS_CATEGORY.WORK_STRUCTURE,
  PROJECT_SETTINGS_CATEGORY.EXECUTION,
];

export const PROJECT_SETTINGS_CATEGORY_I18N_LABELS: Record<PROJECT_SETTINGS_CATEGORY, string> = {
  [PROJECT_SETTINGS_CATEGORY.GENERAL]: "General",
  [PROJECT_SETTINGS_CATEGORY.FEATURES]: "Features",
  [PROJECT_SETTINGS_CATEGORY.WORK_STRUCTURE]: "Work structure",
  [PROJECT_SETTINGS_CATEGORY.EXECUTION]: "Execution",
};

export const PROJECT_SETTINGS: Record<TProjectSettingsTabs, TProjectSettingsItem> = {
  general: {
    key: "general",
    i18n_label: "General",
    href: ``,
    access: [EUserProjectRoles.ADMIN, EUserProjectRoles.MEMBER, EUserProjectRoles.GUEST],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/`,
  },
  members: {
    key: "members",
    i18n_label: "Members",
    href: `/members`,
    access: [EUserProjectRoles.ADMIN, EUserProjectRoles.MEMBER, EUserProjectRoles.GUEST],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/members/`,
  },
  features_cycles: {
    key: "features_cycles",
    i18n_label: "Cycles",
    href: `/features/cycles`,
    access: [EUserProjectRoles.ADMIN],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/features/cycles/`,
  },
  features_modules: {
    key: "features_modules",
    i18n_label: "Modules",
    href: `/features/modules`,
    access: [EUserProjectRoles.ADMIN],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/features/modules/`,
  },
  features_views: {
    key: "features_views",
    i18n_label: "Views",
    href: `/features/views`,
    access: [EUserProjectRoles.ADMIN],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/features/views/`,
  },
  features_pages: {
    key: "features_pages",
    i18n_label: "Pages",
    href: `/features/pages`,
    access: [EUserProjectRoles.ADMIN],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/features/pages/`,
  },
  features_intake: {
    key: "features_intake",
    i18n_label: "Intake",
    href: `/features/intake`,
    access: [EUserProjectRoles.ADMIN],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/features/intake/`,
  },
  states: {
    key: "states",
    i18n_label: "States",
    href: `/states`,
    access: [EUserProjectRoles.ADMIN, EUserProjectRoles.MEMBER],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/states/`,
  },
  labels: {
    key: "labels",
    i18n_label: "Labels",
    href: `/labels`,
    access: [EUserProjectRoles.ADMIN, EUserProjectRoles.MEMBER],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/labels/`,
  },
  estimates: {
    key: "estimates",
    i18n_label: "Estimates",
    href: `/estimates`,
    access: [EUserProjectRoles.ADMIN],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/estimates/`,
  },
  automations: {
    key: "automations",
    i18n_label: "Automations",
    href: `/automations`,
    access: [EUserProjectRoles.ADMIN],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/automations/`,
  },
  github: {
    key: "github",
    i18n_label: "Repository",
    href: `/github`,
    access: [EUserProjectRoles.ADMIN],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/github/`,
  },
  schedulers: {
    key: "schedulers",
    i18n_label: "Schedulers",
    href: `/schedulers`,
    // Project admin only — matches the github / automations gate. Members
    // can view the workspace catalog at Workspace → Schedulers; the
    // per-project install surface adds nothing for non-admins since
    // every action on it requires admin privileges anyway.
    access: [EUserProjectRoles.ADMIN],
    highlight: (pathname: string, baseUrl: string) => pathname === `${baseUrl}/schedulers/`,
  },
};

export const PROJECT_SETTINGS_FLAT_MAP: TProjectSettingsItem[] = Object.values(PROJECT_SETTINGS);

export const GROUPED_PROJECT_SETTINGS: Record<PROJECT_SETTINGS_CATEGORY, TProjectSettingsItem[]> = {
  [PROJECT_SETTINGS_CATEGORY.GENERAL]: [PROJECT_SETTINGS["general"], PROJECT_SETTINGS["members"]],
  [PROJECT_SETTINGS_CATEGORY.FEATURES]: [
    PROJECT_SETTINGS["features_cycles"],
    PROJECT_SETTINGS["features_modules"],
    PROJECT_SETTINGS["features_views"],
    PROJECT_SETTINGS["features_pages"],
    PROJECT_SETTINGS["features_intake"],
  ],
  [PROJECT_SETTINGS_CATEGORY.WORK_STRUCTURE]: [
    PROJECT_SETTINGS["states"],
    PROJECT_SETTINGS["labels"],
    PROJECT_SETTINGS["estimates"],
  ],
  [PROJECT_SETTINGS_CATEGORY.EXECUTION]: [
    PROJECT_SETTINGS["automations"],
    PROJECT_SETTINGS["github"],
    PROJECT_SETTINGS["schedulers"],
  ],
};
