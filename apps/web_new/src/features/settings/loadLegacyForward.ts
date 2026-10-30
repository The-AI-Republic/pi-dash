// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loader for the workspace-less legacy settings addresses (PROF-026;
// main#670). Reads gated because the forwards touch the signed-in user's
// settings; then resolves the tab's place inside the current workspace.
// Kept beside the guard it shares, and loaded with the forward's chunk so
// the initial bundle never pays for it.

import type { QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";

import { settingsQueryOptions, workspacesQueryOptions } from "../../core/session/index.js";
import { legacySettingsRedirect } from "./lib/legacyRedirect.js";
import { enforceProfileSettingsGate } from "./guard.js";

/**
 * The final address a legacy pathname should land on. Returns null when
 * the address belongs to no family (the route then answers not-found).
 */
export async function loadLegacySettingsForward(args: {
  queryClient: QueryClient;
  apiClient: ApiClient;
  pathname: string;
  search: string;
}): Promise<string | null> {
  await enforceProfileSettingsGate({ queryClient: args.queryClient, apiClient: args.apiClient, href: args.pathname });
  const [settings, workspaces] = await Promise.all([
    args.queryClient.ensureQueryData(settingsQueryOptions(args.apiClient)).catch(() => null),
    args.queryClient.ensureQueryData(workspacesQueryOptions(args.apiClient)).catch(() => undefined),
  ]);
  return legacySettingsRedirect(args.pathname, args.search, {
    lastSlug: settings?.workspace.last_workspace_slug ?? null,
    fallbackSlug: settings?.workspace.fallback_workspace_slug ?? null,
    workspaces,
  });
}
