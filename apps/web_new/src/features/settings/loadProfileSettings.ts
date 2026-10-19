// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loader body of the profile-settings layout (parity/specs/profile.md §5,
// PROF-009/010). Kept out of the route definition so the signed-in gate
// and the workspace warm load with the settings page rather than with the
// router's first paint; the route file only awaits this module and gets
// the shortcut list back.

import type { QueryClient } from "@tanstack/react-query";
import type { ApiClient, Workspace } from "@pidash/api-client";

import { workspacesQueryOptions } from "../../core/session/index.js";
import { enforceProfileSettingsGate } from "./guard.js";

/** The layout's loader data: the workspace shortcuts it renders. */
export interface ProfileSettingsLayoutData {
  workspaces: Workspace[];
}

/**
 * Gate the page, then warm the shortcut list. A failed warm leaves the
 * sidebar's workspace section empty; it never blocks the settings page.
 */
export async function loadProfileSettingsLayout(args: {
  queryClient: QueryClient;
  apiClient: ApiClient;
  href: string;
}): Promise<ProfileSettingsLayoutData> {
  await enforceProfileSettingsGate(args);
  const workspaces = await args.queryClient
    .ensureQueryData(workspacesQueryOptions(args.apiClient))
    .catch(() => [] as Workspace[]);
  return { workspaces };
}
