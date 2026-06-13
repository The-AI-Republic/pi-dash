/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash imports
import type { TProfileSettingsTabs } from "@pi-dash/types";

export enum PROFILE_SETTINGS_CATEGORY {
  YOUR_PROFILE = "your profile",
  DEVELOPER = "developer",
}

export const PROFILE_SETTINGS_CATEGORIES: PROFILE_SETTINGS_CATEGORY[] = [
  PROFILE_SETTINGS_CATEGORY.YOUR_PROFILE,
  PROFILE_SETTINGS_CATEGORY.DEVELOPER,
];

export const PROFILE_SETTINGS_CATEGORY_I18N_LABELS: Record<PROFILE_SETTINGS_CATEGORY, string> = {
  [PROFILE_SETTINGS_CATEGORY.YOUR_PROFILE]: "Your profile",
  [PROFILE_SETTINGS_CATEGORY.DEVELOPER]: "Developer",
};

export const PROFILE_SETTINGS: Record<
  TProfileSettingsTabs,
  {
    key: TProfileSettingsTabs;
    i18n_label: string;
  }
> = {
  general: {
    key: "general",
    i18n_label: "Profile",
  },
  security: {
    key: "security",
    i18n_label: "Security",
  },
  activity: {
    key: "activity",
    i18n_label: "Activity",
  },
  preferences: {
    key: "preferences",
    i18n_label: "Preferences",
  },
  "ai-assistant": {
    key: "ai-assistant",
    i18n_label: "AI Assistant",
  },
  "auto-project-management": {
    key: "auto-project-management",
    i18n_label: "Auto Project Management",
  },
  notifications: {
    key: "notifications",
    i18n_label: "Notifications",
  },
  "api-tokens": {
    key: "api-tokens",
    i18n_label: "Personal Access Tokens",
  },
};

export const PROFILE_SETTINGS_TABS: TProfileSettingsTabs[] = Object.keys(PROFILE_SETTINGS) as TProfileSettingsTabs[];

export const GROUPED_PROFILE_SETTINGS: Record<
  PROFILE_SETTINGS_CATEGORY,
  { key: TProfileSettingsTabs; i18n_label: string }[]
> = {
  [PROFILE_SETTINGS_CATEGORY.YOUR_PROFILE]: [
    PROFILE_SETTINGS["general"],
    PROFILE_SETTINGS["preferences"],
    PROFILE_SETTINGS["ai-assistant"],
    PROFILE_SETTINGS["auto-project-management"],
    PROFILE_SETTINGS["notifications"],
    PROFILE_SETTINGS["security"],
    PROFILE_SETTINGS["activity"],
  ],
  [PROFILE_SETTINGS_CATEGORY.DEVELOPER]: [PROFILE_SETTINGS["api-tokens"]],
};
