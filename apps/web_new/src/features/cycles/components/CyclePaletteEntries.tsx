// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle palette entries mount (parity/specs/cycles.md §11, CYC-030).
// The detail page renders this beside its content: it registers the
// cycle's contextual palette entries (favorite toggle plus copy-URL)
// through the shell-owned hook while the page is mounted, and
// re-reads the cycle row plus the sidebar favorites after a toggle
// settles so the star, the row and the sidebar agree. Archived cycles
// register nothing; guests keep copy-URL only.

import { useQueryClient } from "@tanstack/react-query";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import * as React from "react";

import { ROLE_GUEST } from "../../../core/session/permissions.js";
import { useCyclePaletteContext } from "../../../shared/shell/contextual/index.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { invalidateAfterCycleWrite } from "../api/invalidate.js";

export interface CyclePaletteEntriesProps {
  workspaceSlug: string;
  projectId: string;
  cycle: Pick<CycleRowData, "id" | "name" | "is_favorite" | "archived_at">;
  /** Caller's project role; null reads as guest level. */
  memberRole: number | null;
  /** True when the project itself is archived. */
  projectArchived: boolean;
}

export function CyclePaletteEntries({
  workspaceSlug,
  projectId,
  cycle,
  memberRole,
  projectArchived,
}: CyclePaletteEntriesProps): null {
  const queryClient = useQueryClient();
  const refresh = React.useCallback(async () => {
    await invalidateAfterCycleWrite(queryClient, workspaceSlug, projectId, cycle.id);
    await queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(workspaceSlug) });
  }, [queryClient, workspaceSlug, projectId, cycle.id]);
  useCyclePaletteContext({
    workspaceSlug,
    projectId,
    cycleId: cycle.id,
    cycleName: cycle.name,
    archived: cycle.archived_at !== null && cycle.archived_at !== undefined,
    favorite: cycle.is_favorite,
    standing: { memberRole: memberRole ?? ROLE_GUEST, archived: projectArchived },
    refresh,
  });
  return null;
}
