// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Fixed product badge (parity/specs/space.md SPC-012). A small corner
// pill linking the product site in a new tab, pinned above board
// content but below transient surfaces (toasts, menus) so a notice
// never slides under an always-on-top brand mark. It mounts on the
// resolved board and on the landing's auth and guidance views (the
// landing mounts come from slice B); the loading, framed, not-found,
// crash and legacy states mount nothing. An explicitly blank site
// address or the disabled flag renders nothing — no empty slot, no
// broken link. The gate driver finds the badge by its test id; the
// wording is new, only the product name carries over.
// Gate DOM contract: a[href target=_blank rel~="noopener"
// data-testid="space-product-badge"], bottom-right pinned.
import * as React from "react";

import { useSpaceEmbedded } from "../lib/spaceEmbed.js";
import { spaceMessage } from "../lib/spaceMessages.js";

/** Product site behind the badge when the build names none. */
export const DEFAULT_SPACE_BADGE_URL = "https://airepublic.com";

/**
 * Badge address for a configured value: the site trimmed, null when the
 * build blanks it (badge off), else the product default. A missing or
 * non-string value counts as absent rather than breaking the link.
 */
export function resolveSpaceBadgeUrl(configured: unknown): string | null {
  if (typeof configured !== "string") return DEFAULT_SPACE_BADGE_URL;
  const trimmed = configured.trim();
  return trimmed === "" ? null : trimmed;
}

export interface SpaceProductBadgeProps {
  /** Force the badge off; mirrors the old disabled flag. */
  disabled?: boolean | undefined;
}

export function SpaceProductBadge({ disabled = false }: SpaceProductBadgeProps): React.ReactElement | null {
  const embedded = useSpaceEmbedded();
  const href = resolveSpaceBadgeUrl(import.meta.env.VITE_WEBSITE_URL);
  if (disabled || embedded || href === null) return null;
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      data-testid="space-product-badge"
      className="text-meta shadow-popover fixed right-(--space-5) bottom-(--space-3) z-40 flex items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-2) py-(--space-1) text-(--text-muted)"
    >
      <span>{spaceMessage("badge.lead")}</span>
      <span className="font-semibold text-(--text)">{spaceMessage("badge.product")}</span>
    </a>
  );
}
