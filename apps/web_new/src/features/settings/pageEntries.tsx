// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Registration side of the profile-settings seam (parity/specs/profile.md
// §5). Every tab the product owns outside a sibling feature registers
// here; sibling slices register their own tabs from their lazy route
// modules. Account settings arrive through loadAccountSettingsTabs.ts, so
// these bodies sit in their own chunk and not in the first paint.

import * as React from "react";

import { AssistantSettingsTab } from "../profile-assistant/index.js";
import { IntegrationsSettingsTab } from "../profile-integrations/index.js";
import { UpkeepSettingsTab } from "../profile-upkeep/index.js";
import { registerCloudAssistantSettings } from "../cloud-assistant/index.js";
import { ApiTokensTab } from "./components/ApiTokensTab.js";
import { GeneralSettingsTab } from "./components/GeneralSettingsTab.js";
import { NotificationsTab } from "./components/NotificationsTab.js";
import { SecuritySettingsTab } from "./components/SecuritySettingsTab.js";
import { registerProfileSettingsPage } from "./lib/registry.js";
import { OwnActivityTab } from "./ownActivity/components/OwnActivityTab.js";
import { PreferencesTab } from "./preferences/components/PreferencesTab.js";

export function registerBuiltInSettingsTabs(): () => void {
  const unregisterGeneral = registerProfileSettingsPage("general", () => <GeneralSettingsTab />);
  const unregisterPreferences = registerProfileSettingsPage("preferences", () => <PreferencesTab />);
  const unregisterSecurity = registerProfileSettingsPage("security", () => <SecuritySettingsTab />);
  const unregisterNotifications = registerProfileSettingsPage("notifications", () => <NotificationsTab />);
  // The token body only adopts the workspace-settings page (§9, §41); its
  // content stays in the workspace-settings chunk.
  const unregisterApiTokens = registerProfileSettingsPage("api-tokens", () => <ApiTokensTab />);
  const unregisterActivity = registerProfileSettingsPage("activity", () => <OwnActivityTab />);
  const unregisterAssistant = registerProfileSettingsPage("ai-assistant", () => <AssistantSettingsTab />);
  const unregisterUpkeep = registerProfileSettingsPage("auto-project-management", () => <UpkeepSettingsTab />);
  const unregisterIntegrations = registerProfileSettingsPage("integrations", () => <IntegrationsSettingsTab />);
  // Registered last so an edition that declares the section supersedes the
  // community assistant body rather than being overwritten by it.
  const unregisterCloudAssistant = registerCloudAssistantSettings();
  return () => {
    unregisterGeneral();
    unregisterPreferences();
    unregisterSecurity();
    unregisterNotifications();
    unregisterApiTokens();
    unregisterCloudAssistant();
    unregisterActivity();
    unregisterAssistant();
    unregisterUpkeep();
    unregisterIntegrations();
  };
}

registerBuiltInSettingsTabs();
