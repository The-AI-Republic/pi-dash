/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import {
  Activity,
  Bell,
  CircleUser,
  KeyRound,
  LockIcon,
  Plug,
  type LucideIcon,
  RefreshCw,
  Settings2,
  Sparkles,
} from "lucide-react";
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

// The icon is co-located with each tab so the sidebar can render `item.icon`
// directly — there is no separate key→icon lookup table that can drift out of
// sync when a tab is added (which previously caused an undefined component →
// React #130 crash, especially in the cloud overlay's forked sidebar).
export const PROFILE_SETTINGS: Record<
  TProfileSettingsTabs,
  {
    key: TProfileSettingsTabs;
    i18n_label: string;
    icon: LucideIcon;
  }
> = {
  general: {
    key: "general",
    i18n_label: "Profile",
    icon: CircleUser,
  },
  security: {
    key: "security",
    i18n_label: "Security",
    icon: LockIcon,
  },
  activity: {
    key: "activity",
    i18n_label: "Activity",
    icon: Activity,
  },
  preferences: {
    key: "preferences",
    i18n_label: "Preferences",
    icon: Settings2,
  },
  "ai-assistant": {
    key: "ai-assistant",
    i18n_label: "AI Assistant",
    icon: Sparkles,
  },
  "auto-project-management": {
    key: "auto-project-management",
    i18n_label: "Auto Project Management",
    icon: RefreshCw,
  },
  notifications: {
    key: "notifications",
    i18n_label: "Notifications",
    icon: Bell,
  },
  integrations: {
    key: "integrations",
    i18n_label: "Integrations",
    icon: Plug,
  },
  "api-tokens": {
    key: "api-tokens",
    i18n_label: "Personal Access Tokens",
    icon: KeyRound,
  },
};

export const PROFILE_SETTINGS_TABS: TProfileSettingsTabs[] = Object.keys(PROFILE_SETTINGS) as TProfileSettingsTabs[];

export const GROUPED_PROFILE_SETTINGS: Record<
  PROFILE_SETTINGS_CATEGORY,
  { key: TProfileSettingsTabs; i18n_label: string; icon: LucideIcon }[]
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
  [PROFILE_SETTINGS_CATEGORY.DEVELOPER]: [PROFILE_SETTINGS["integrations"], PROFILE_SETTINGS["api-tokens"]],
};
