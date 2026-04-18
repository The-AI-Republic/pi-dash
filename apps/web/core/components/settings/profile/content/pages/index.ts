/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { lazy } from "react";
// pi dash imports
import type { TProfileSettingsTabs } from "@pi-dash/types";

export const PROFILE_SETTINGS_PAGES_MAP: Record<TProfileSettingsTabs, React.LazyExoticComponent<React.FC>> = {
  general: lazy(() => import("./general").then((m) => ({ default: m.GeneralProfileSettings }))),
  preferences: lazy(() => import("./preferences").then((m) => ({ default: m.PreferencesProfileSettings }))),
  notifications: lazy(() => import("./notifications").then((m) => ({ default: m.NotificationsProfileSettings }))),
  security: lazy(() => import("./security").then((m) => ({ default: m.SecurityProfileSettings }))),
  activity: lazy(() => import("./activity").then((m) => ({ default: m.ActivityProfileSettings }))),
  "api-tokens": lazy(() => import("./api-tokens").then((m) => ({ default: m.APITokensProfileSettings }))),
};
