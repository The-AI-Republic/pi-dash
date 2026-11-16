// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle favorite star (parity/specs/cycles.md §11, CYC-029). The row's
// mark as a working toggle: the filled/hollow state follows the write
// (optimistic while it flies, settled by the re-read), and the star
// pins its progress state while the write is in flight. Guests and
// archived rows never mount this — the caller gates it.

import type { CycleRow as CycleRowData } from "@pidash/api-client";
import { Star } from "lucide-react";
import * as React from "react";

import { useCycleFavorite } from "../api/cycleFavorite.js";

export interface CycleFavoriteStarProps {
  workspaceSlug: string;
  projectId: string;
  cycle: Pick<CycleRowData, "id" | "name" | "is_favorite">;
}

export function CycleFavoriteStar({ workspaceSlug, projectId, cycle }: CycleFavoriteStarProps): React.ReactElement {
  const { starred, pending, toggle } = useCycleFavorite({ workspaceSlug, projectId, cycle });
  return (
    <button
      type="button"
      data-testid="cycle-favorite-marker"
      data-favorite={starred ? "true" : "false"}
      aria-label={starred ? `Remove ${cycle.name} from favorites` : `Add ${cycle.name} to favorites`}
      aria-pressed={starred}
      aria-busy={pending}
      disabled={pending}
      onClick={toggle}
      className="inline-flex shrink-0 cursor-pointer disabled:cursor-wait disabled:opacity-60"
    >
      <Star
        size={16}
        strokeWidth={1.5}
        aria-hidden="true"
        className={starred ? "fill-(--accent) text-(--accent)" : "text-(--text-muted)"}
      />
    </button>
  );
}
