// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Decorative marketing backdrop (CLOUD-004): the monospace world map from
// the public marketing asset, painted as a faint tint across the page.
// The asset is a mask rather than an <img>, so the tint follows the text
// colour token and reads in light and dark themes alike. Hidden from
// assistive technology, no behavior.
import * as React from "react";

/** Where the map lives once the site is built (served from public/). */
export const WORLD_MAP_ASSET = "/world-map.svg";

/** The paint mask: one image, centred, scaled to fit the viewport. */
export const WORLD_MAP_MASK: React.CSSProperties = {
  WebkitMaskImage: `url("${WORLD_MAP_ASSET}")`,
  maskImage: `url("${WORLD_MAP_ASSET}")`,
  WebkitMaskRepeat: "no-repeat",
  maskRepeat: "no-repeat",
  WebkitMaskPosition: "center",
  maskPosition: "center",
  WebkitMaskSize: "contain",
  maskSize: "contain",
};

export function MarketingBackdrop(): React.ReactElement {
  return (
    <div
      aria-hidden="true"
      data-marketing-backdrop=""
      style={WORLD_MAP_MASK}
      className="pointer-events-none fixed inset-0 z-0 bg-(--text) opacity-[0.055]"
    />
  );
}
