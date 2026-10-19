// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Registration side of the profile-settings seam (parity/specs/profile.md
// §5). Every tab the product owns outside a sibling feature registers
// here: the shell imports this module so the general and security bodies
// are present on a direct visit; sibling slices register their own tabs
// from their lazy route modules.

import * as React from "react";

import { GeneralSettingsTab } from "./components/GeneralSettingsTab.js";
import { NotificationsTab } from "./components/NotificationsTab.js";
import { SecuritySettingsTab } from "./components/SecuritySettingsTab.js";
import { registerProfileSettingsPage } from "./lib/registry.js";

export function registerBuiltInSettingsTabs(): () => void {
  const unregisterGeneral = registerProfileSettingsPage("general", () => <GeneralSettingsTab />);
  const unregisterSecurity = registerProfileSettingsPage("security", () => <SecuritySettingsTab />);
  const unregisterNotifications = registerProfileSettingsPage("notifications", () => <NotificationsTab />);
  return () => {
    unregisterGeneral();
    unregisterSecurity();
    unregisterNotifications();
  };
}

registerBuiltInSettingsTabs();
