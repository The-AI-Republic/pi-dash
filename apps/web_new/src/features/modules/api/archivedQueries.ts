// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived-modules read (parity/specs/modules.md §7.3, MOD-027). The
// archived screen owns its own list read: the live list does not carry
// archived rows. It sits under `moduleKeys.archivedList`, which every
// module write already refreshes (§13.6). Fetch-on-mount only, like every
// other module read. The peek's archived detail read is the sidebar's
// (`moduleSidebarDetailQueryOptions` with `archived`), so the panel and
// the sidebar never diverge into two detail reads.

import { queryOptions } from "@tanstack/react-query";
import { listArchivedModules, type ApiClient } from "@pidash/api-client";

import { fetchOnMountOptions } from "../../issues/index.js";
import { moduleKeys } from "./keys.js";

/** The project's archived modules, flat, in the server's own order. */
export function archivedModulesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions(
    fetchOnMountOptions({
      queryKey: moduleKeys.archivedList(workspaceSlug, projectId),
      queryFn: () => listArchivedModules(client, workspaceSlug, projectId),
    })
  );
}
