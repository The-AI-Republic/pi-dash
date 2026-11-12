// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the settings feature. Sibling features mount their tab
// bodies through this barrel rather than reaching into lib/registry, and
// link to account settings with the address helper it re-exports.

export { accountSettingsHref, accountSettingsGeneralHref } from "./lib/accountHref.js";
export { registerProfileSettingsPage, resetProfileSettingsPages } from "./lib/registry.js";
export type { ProfileSettingsPageComponent } from "./lib/registry.js";
export type { ProfileSettingsTabId } from "./lib/tabNavigation.js";
