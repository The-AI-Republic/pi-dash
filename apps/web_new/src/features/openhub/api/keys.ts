// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Apps catalog query keys (Data layer; written from
// parity/specs/cloud.md §5, CLOUD-022/023). Workspace-scoped like every
// feature key; the marketplace key carries the settled query plus the
// cursor, so a new query can never page into the previous result set.
import { workspaceScope } from "../../../core/query/client.js";

export const openhubKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "openhub"] as const,
  status: (workspaceSlug: string) => [...openhubKeys.all(workspaceSlug), "status"] as const,
  marketplace: (workspaceSlug: string) => [...openhubKeys.all(workspaceSlug), "marketplace"] as const,
  marketplacePage: (workspaceSlug: string, query: string, cursor: string | null) =>
    [...openhubKeys.marketplace(workspaceSlug), query, cursor] as const,
};
