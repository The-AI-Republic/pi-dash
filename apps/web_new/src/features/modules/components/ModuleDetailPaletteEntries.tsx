// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail palette entries mount (shell SHELL-091). The detail
// screen renders this beside its content: it registers the module's
// contextual palette entries (member and status pickers, favorite
// toggle plus copy-URL) through the shell-owned hook while the page is
// mounted, and re-reads the module plus the sidebar favorites after a
// write settles so the rows and the sidebar agree. Archived modules
// and guests keep copy-URL only.

import { useQueryClient } from "@tanstack/react-query";
import type { ModuleRow as ModuleRowData } from "@pidash/api-client";
import * as React from "react";

import { ROLE_GUEST } from "../../../core/session/permissions.js";
import { useModulePaletteContext } from "../../../shared/shell/contextual/index.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { invalidateAfterModuleWrite } from "../api/invalidate.js";

export interface ModuleDetailPaletteEntriesProps {
  workspaceSlug: string;
  projectId: string;
  module: Pick<ModuleRowData, "id" | "name" | "status" | "member_ids" | "is_favorite" | "archived_at">;
  /** Caller's project role; null reads as guest level. */
  memberRole: number | null;
  /** True when the project itself is archived. */
  projectArchived: boolean;
}

export function ModuleDetailPaletteEntries({
  workspaceSlug,
  projectId,
  module,
  memberRole,
  projectArchived,
}: ModuleDetailPaletteEntriesProps): null {
  const queryClient = useQueryClient();
  const refresh = React.useCallback(async () => {
    await invalidateAfterModuleWrite(queryClient, workspaceSlug, projectId, module.id);
    await queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(workspaceSlug) });
  }, [queryClient, workspaceSlug, projectId, module.id]);
  useModulePaletteContext({
    workspaceSlug,
    projectId,
    moduleId: module.id,
    moduleName: module.name,
    archived: module.archived_at !== null && module.archived_at !== undefined,
    status: module.status,
    memberIds: module.member_ids,
    favorite: module.is_favorite,
    standing: { memberRole: memberRole ?? ROLE_GUEST, archived: projectArchived },
    refresh,
  });
  return null;
}
