// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail header favourite star (parity/specs/pages.md §7.2, PAGE-056). A
// thin face over the shared favourite write helper (§13.1): the same
// optimistic flip, rollback and first-add panel prompt as the list row
// star, with the header's own pinned labels. Guests never mount this —
// the screen decides.

import { Star } from "lucide-react";
import * as React from "react";

import type { PageRow as PageRowData } from "@pidash/api-client";

import { usePageFavorite } from "../api/mutations.js";

/** Pinned accessible names; the oracle reads both literally. */
export const PAGE_DETAIL_FAVORITE_ADD_LABEL = "Add to favorites";
export const PAGE_DETAIL_FAVORITE_REMOVE_LABEL = "Remove favorite";

export interface PageDetailFavoriteStarProps {
  workspaceSlug: string;
  projectId: string;
  page: Pick<PageRowData, "id" | "name" | "is_favorite">;
}

export function PageDetailFavoriteStar({
  workspaceSlug,
  projectId,
  page,
}: PageDetailFavoriteStarProps): React.ReactElement {
  const { starred, pending, toggle } = usePageFavorite({ workspaceSlug, projectId, page });
  return (
    <button
      type="button"
      data-testid="page-detail-star"
      aria-label={starred ? PAGE_DETAIL_FAVORITE_REMOVE_LABEL : PAGE_DETAIL_FAVORITE_ADD_LABEL}
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
