/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TPowerKCommandGroup } from "../../core/types";

export const POWER_K_GROUP_PRIORITY: Record<TPowerKCommandGroup, number> = {
  contextual: 1,
  create: 2,
  navigation: 3,
  general: 7,
  settings: 8,
  account: 9,
  miscellaneous: 10,
  preferences: 11,
  help: 12,
};

export const POWER_K_GROUP_I18N_TITLES: Record<TPowerKCommandGroup, string> = {
  contextual: "Contextual",
  navigation: "Navigate",
  create: "Create",
  general: "General",
  settings: "Settings",
  help: "Help",
  account: "Account",
  miscellaneous: "Miscellaneous",
  preferences: "Preferences",
};
