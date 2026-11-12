// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-settings route identity (parity/specs/workspace-settings.md
// §1). Split out of sections.ts (NEWFRONT-852, size gate): the gated
// shell reads only the layout route id on every first paint, while the
// section catalog must stay in lazy chunks. sections.ts re-exports both,
// so existing importers keep working.

/**
 * The file-route id of the workspace-settings layout. Addresses beneath
 * it are the six section pages, the webhook detail and the unknown-section
 * catch-all; sibling settings areas (project settings beneath
 * `settings/projects`, the workspace-less profile settings) are other
 * routes and keep their own chrome.
 */
export const WORKSPACE_SETTINGS_ROUTE_ID = "/$ws/_gated/settings/_workspace";

/** True for the settings layout route and every route beneath it. */
export function isWorkspaceSettingsRouteId(routeId: string): boolean {
  return routeId === WORKSPACE_SETTINGS_ROUTE_ID || routeId.startsWith(`${WORKSPACE_SETTINGS_ROUTE_ID}/`);
}
