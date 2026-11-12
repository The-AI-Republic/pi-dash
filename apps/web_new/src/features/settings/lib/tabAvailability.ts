// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Address guard for a profile tab the instance may withhold
// (parity/specs/cloud.md §8, CLOUD-035). Reads the instance marker first
// so a bookmarked password address forwards to the general tab on a
// cloud-managed instance instead of rendering fields the account home
// owns. A failed read counts as unknown and serves the tab unchanged:
// the address a self-managed operator needs never depends on one
// request succeeding.

import type { QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";

import { instanceConfigQueryOptions } from "../../../core/session/queries.js";
import type { ProfileSettingsTabId } from "./tabNavigation.js";
import { isProfileSettingsTabServed } from "./visibility.js";

/** The instance's self-managed marker, or undefined when it cannot be read. */
export async function loadSelfManagedMarker(
  queryClient: QueryClient,
  apiClient: ApiClient
): Promise<boolean | undefined> {
  try {
    const config = await queryClient.ensureQueryData(instanceConfigQueryOptions(apiClient));
    return config.is_self_managed;
  } catch {
    return undefined;
  }
}

/** True when this instance serves the tab's address. */
export async function isTabAddressServed(args: {
  queryClient: QueryClient;
  apiClient: ApiClient;
  tab: ProfileSettingsTabId;
}): Promise<boolean> {
  // Only a gated tab needs the answer; an ordinary address never waits on it.
  if (!tabHasInstanceGate(args.tab)) return true;
  // No instance read for a self-managed-only tab: the address stands.
  const selfManaged = await loadSelfManagedMarker(args.queryClient, args.apiClient);
  return isProfileSettingsTabServed(args.tab, selfManaged ?? true);
}

/** True when any instance can withhold this tab's address, so guards read first. */
export function tabHasInstanceGate(tab: ProfileSettingsTabId): boolean {
  return !isProfileSettingsTabServed(tab, false) || !isProfileSettingsTabServed(tab, true);
}
