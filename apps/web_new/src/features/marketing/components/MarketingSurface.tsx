// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public marketing surface (CLOUD-003/004/005, parity/specs/cloud.md §1):
// the shared frame, the page's crawler metadata and the page body. The
// layout route loads this module lazily, so the marketing copy, frame and
// icons stay out of the initial bundle for every other area. The surface
// picks the body from the address itself: the four marketing routes carry
// only their declarations, so no page module is reachable from the eager
// router.
import * as React from "react";

import type { PublicMarketingPath } from "../../../core/edition/index.js";
import { MarketingFrame } from "./MarketingFrame.js";
import { MarketingHead } from "./MarketingHead.js";
import { MarketingHome } from "./MarketingHome.js";
import { MarketingIntro } from "./MarketingIntro.js";

export interface MarketingSurfaceProps {
  /** Marketing address being shown, without a trailing slash. */
  path: PublicMarketingPath;
}

export default function MarketingSurface({ path }: MarketingSurfaceProps): React.ReactElement {
  return (
    <MarketingFrame>
      <MarketingHead path={path} />
      {path === "/home" ? <MarketingHome /> : <MarketingIntro path={path} />}
    </MarketingFrame>
  );
}
