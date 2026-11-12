// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-board document head (SPC-007). The claim is built from the public
// meta payload: board name and summary become the title, description and
// social tags, with the generic product tags as the fallback. Cover
// images are never emitted — not even when the payload carries one —
// and crawlers are always told to stay out (SPC-008).

import type { SpaceBoardMeta } from "@pidash/api-client";

import type { HeadTag } from "../../../shared/shell/document.js";

/** Generic title when the board carries no usable name. */
export const SPACE_HEAD_TITLE_FALLBACK = "Pi Dash";

/** Generic description when the board carries no usable summary. */
export const SPACE_HEAD_DESCRIPTION_FALLBACK = "Projects, issues and agents in one place.";

/** Crawler directive for every space page. */
export const SPACE_HEAD_ROBOTS = "noindex, nofollow";

export interface SpaceBoardHeadClaim {
  title: string;
  robots: string;
  tags: HeadTag[];
}

function displayText(value: string | undefined, fallback: string): string {
  return typeof value === "string" && value.trim() !== "" ? value : fallback;
}

export function buildSpaceBoardHead(meta: SpaceBoardMeta | null | undefined): SpaceBoardHeadClaim {
  const title = displayText(meta?.name, SPACE_HEAD_TITLE_FALLBACK);
  const description = displayText(meta?.description, SPACE_HEAD_DESCRIPTION_FALLBACK);
  return {
    title,
    robots: SPACE_HEAD_ROBOTS,
    tags: [
      { tag: "meta", attributes: { name: "description", content: description } },
      { tag: "meta", attributes: { property: "og:title", content: title } },
      { tag: "meta", attributes: { property: "og:description", content: description } },
      { tag: "meta", attributes: { property: "og:type", content: "website" } },
      { tag: "meta", attributes: { name: "twitter:card", content: "summary" } },
      { tag: "meta", attributes: { name: "twitter:title", content: title } },
      { tag: "meta", attributes: { name: "twitter:description", content: description } },
    ],
  };
}
