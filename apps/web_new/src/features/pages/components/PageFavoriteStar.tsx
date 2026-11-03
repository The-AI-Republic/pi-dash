// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row favourite star (parity/specs/pages.md §3.5 and §13.1, PAGE-009). A
// thin face over the shared favourite write helper: the filled/hollow
// state follows the cached row (flipped optimistically, settled by the
// re-read), the star pins its progress while the write flies, and the
// first add opens the sidebar's favourites section through the helper.
// Guests never mount this — the row decides.

import { Star } from "lucide-react";
import * as React from "react";

import type { PageRow as PageRowData } from "@pidash/api-client";

import { usePageFavorite } from "../api/mutations.js";
import { pageDisplayTitle } from "../lib/pageList.js";

export interface PageFavoriteStarProps {
  workspaceSlug: string;
  projectId: string;
  page: Pick<PageRowData, "id" | "name" | "is_favorite">;
}

export function PageFavoriteStar({ workspaceSlug, projectId, page }: PageFavoriteStarProps): React.ReactElement {
  const { starred, pending, toggle } = usePageFavorite({ workspaceSlug, projectId, page });
  const title = pageDisplayTitle(page.name);
  return (
    <button
      type="button"
      data-testid="page-row-star"
      aria-label={starred ? `Remove ${title} from favorites` : `Add ${title} to favorites`}
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
