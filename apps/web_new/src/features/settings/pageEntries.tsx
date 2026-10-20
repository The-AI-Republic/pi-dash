// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Registration side of the profile-settings seam (parity/specs/profile.md
// §5). Every tab the product owns outside a sibling feature registers
// here: the shell imports this module so the general, security, token and
// notifications bodies are present on a direct visit; sibling slices
// register their own tabs from their lazy route modules. The settings
// layout also calls `useSessionExpiredRedirect` from this entry module,
// because settings is rendered outside the app shell and the gated
// workspace tree, so nothing else would notice an expired session here.

import * as React from "react";

import { AssistantSettingsTab } from "../profile-assistant/index.js";
import { IntegrationsSettingsTab } from "../profile-integrations/index.js";
import { UpkeepSettingsTab } from "../profile-upkeep/index.js";
import { useSessionExpiredRedirect } from "../../shared/shell/expiredRedirect.js";
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

/** Keeps the settings page on the session-expiry path like the app shell. */
export function useProfileSettingsSessionGuard(): void {
  useSessionExpiredRedirect();
}

registerBuiltInSettingsTabs();
