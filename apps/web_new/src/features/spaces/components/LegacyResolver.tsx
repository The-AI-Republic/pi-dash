// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy project link (SPC-002). The resolver is the route component: it
// holds a full-viewport spinner while the anchor resolves, then replaces
// the address with the canonical board address carrying only board/peekId.
// Every refusal lands on the fixed generic not-found address, never on the
// anchor illustration.

import { Spinner } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { spaceKeys } from "../api/keys.js";
import { resolveLegacySpaceAnchor } from "../api/queries.js";
import { legacyForwardHref, SPACE_GENERIC_NOT_FOUND, type SpaceBoardSearch } from "../lib/address.js";

export function LegacyResolver({
  workspaceSlug,
  projectId,
  search,
}: {
  workspaceSlug: string;
  projectId: string;
  search: SpaceBoardSearch;
}): React.ReactElement {
  const navigate = useNavigate();
  const query = useQuery({
    queryKey: spaceKeys.legacy(workspaceSlug, projectId),
    queryFn: () => resolveLegacySpaceAnchor(spaceRuntime.client, workspaceSlug, projectId),
    retry: false,
    staleTime: 30_000,
  });
  React.useEffect(() => {
    if (query.isPending) return;
    if (query.data === null || query.data === undefined) {
      void navigate({ href: SPACE_GENERIC_NOT_FOUND, replace: true });
      return;
    }
    void navigate({ href: legacyForwardHref(query.data, search), replace: true });
  }, [navigate, query.data, query.isPending, search]);
  return (
    <div
      data-testid="space-legacy-loading"
      role="status"
      aria-label="Resolving board"
      className="flex min-h-screen items-center justify-center bg-(--bg)"
    >
      <Spinner size="large" label="Resolving board" />
    </div>
  );
}

export type { SpaceBoardSearch };
