// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page detail palette entries mount (shell SHELL-091). The detail
// screen renders this beside its content: it registers the page's
// contextual palette entries (lock, visibility, archive and favorite
// toggles plus copy-URL, each gated on its own capability) through the
// shell-owned hook while the page is mounted, and re-reads the page
// plus the sidebar favorites after a write settles so the header, the
// list and the sidebar agree. Viewers without standing keep copy-URL
// only.

import { useQueryClient } from "@tanstack/react-query";
import type { PageRow as PageRowData } from "@pidash/api-client";
import * as React from "react";

import { usePagePaletteContext } from "../../../shared/shell/contextual/index.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { pageKeys } from "../api/keys.js";

export interface PageDetailPaletteEntriesProps {
  workspaceSlug: string;
  projectId: string;
  page: Pick<PageRowData, "id" | "name" | "access" | "is_favorite" | "is_locked" | "archived_at" | "owned_by">;
  /** True when the viewer owns the page. */
  isOwner: boolean;
  /** Caller's highest project role; null when outside the project. */
  highestProjectRole: number | null;
}

export function PageDetailPaletteEntries({
  workspaceSlug,
  projectId,
  page,
  isOwner,
  highestProjectRole,
}: PageDetailPaletteEntriesProps): null {
  const queryClient = useQueryClient();
  const refresh = React.useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: pageKeys.list(workspaceSlug, projectId) });
    await queryClient.invalidateQueries({ queryKey: pageKeys.detail(workspaceSlug, projectId, page.id) });
    await queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(workspaceSlug) });
  }, [queryClient, workspaceSlug, projectId, page.id]);
  usePagePaletteContext({
    workspaceSlug,
    projectId,
    pageId: page.id,
    pageName: page.name,
    locked: page.is_locked,
    access: page.access,
    archived: page.archived_at !== null,
    favorite: page.is_favorite,
    isOwner,
    highestProjectRole,
    refresh,
  });
  return null;
}
