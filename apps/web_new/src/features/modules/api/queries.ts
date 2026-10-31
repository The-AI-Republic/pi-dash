// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module reads the list shell owns (parity/specs/modules.md §2.1, §3.1,
// §8.5, §13.1; MOD-039). The list is one bare-array read per project and
// fetches on mount only: another member's change appears on the next load,
// while the acting user's own writes refresh through the invalidation
// helpers. Detail, links and work-item reads belong to the slices that
// render them; their keys already live in keys.ts.

import { queryOptions } from "@tanstack/react-query";
import { listProjectModules, type ApiClient } from "@pidash/api-client";

import { fetchOnMountOptions } from "../../issues/index.js";
import { moduleKeys } from "./keys.js";

/** The project's non-archived modules: full rows for the list, board and timeline. */
export function modulesListQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions(
    fetchOnMountOptions({
      queryKey: moduleKeys.list(workspaceSlug, projectId),
      queryFn: () => listProjectModules(client, workspaceSlug, projectId),
    })
  );
}
