// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Published board address (SPC-001). The search schema settles only the
// board parameters this slice consumes; unknown values fall back without
// throwing. Pasted slash-less addresses settle on the canonical
// trailing-slash form on the first client navigation, keeping their search
// and fragment. The full view/filter contract belongs to slices E/F and
// mounts onto this route.

import { createFileRoute, useLocation, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { BoardScreen } from "../../../features/spaces/components/BoardScreen.js";
import { parseSpaceBoardSearch, type SpaceBoardSearch } from "../../../features/spaces/lib/address.js";

export const Route = createFileRoute("/spaces/issues/$anchor")({
  validateSearch: parseSpaceBoardSearch,
  component: function SpaceBoardRoute() {
    const { anchor } = Route.useParams();
    const search = Route.useSearch();
    useTrailingSlashAddress();
    return <BoardScreen anchor={anchor} search={search} />;
  },
});

/**
 * Settle a pasted slash-less board address on the canonical trailing-slash
 * form with a replace, keeping search and fragment byte for byte. This
 * runs as a mounted effect — never as a beforeLoad redirect to the slashed
 * self, which re-triggers beforeLoad forever and OOMs the tab
 * (NEWFRONT-662). The layout settle (useSettledSpaceLayout) may replace
 * again right after; both targets are fixed points, so the order between
 * them cannot loop.
 */
function useTrailingSlashAddress(): void {
  const navigate = useNavigate();
  const location = useLocation();
  const hash = location.hash === "" ? "" : `#${location.hash}`;
  const href = location.pathname.endsWith("/") ? null : `${location.pathname}/${location.searchStr}${hash}`;
  const navigateRef = React.useRef(navigate);
  navigateRef.current = navigate;
  React.useEffect(() => {
    if (href === null) return;
    void navigateRef.current({ href, replace: true });
  }, [href]);
}

export type { SpaceBoardSearch };
