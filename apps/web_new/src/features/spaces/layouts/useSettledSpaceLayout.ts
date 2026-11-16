// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// SPC-021/022: the board address owns the layout. A missing or unrecognised
// `board` settles on the first offered layout and the address is corrected —
// with a replace, so settling adds no history entry and Back cannot loop.
// Choosing a layout writes it with a push, so Back returns to the previous
// view. The loaded issues are untouched by either.

import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { normalizeBoardHref, type SpaceBoardSearch, type SpaceLayout } from "../lib/address.js";
import { enabledSpaceLayouts, resolveSpaceLayout } from "./model.js";

export interface SettledSpaceLayout {
  /** The layouts the switcher offers, in board order. */
  offered: SpaceLayout[];
  /** The layout the board renders (and the address must name). */
  active: SpaceLayout;
  /** Switch layouts: pushes one address naming the new view. */
  select: (layout: SpaceLayout) => void;
}

/**
 * Resolve the offered/active layout for one anchor and address, correcting
 * the address when it does not name an offered layout.
 */
export function useSettledSpaceLayout(
  anchor: string,
  search: SpaceBoardSearch,
  viewProps: Record<string, boolean> | undefined
): SettledSpaceLayout {
  const navigate = useNavigate();
  const offered = React.useMemo(() => enabledSpaceLayouts(viewProps ?? {}), [viewProps]);
  const active = resolveSpaceLayout(search.board, offered);
  const needsRewrite = search.board !== active;
  // The active layout is a value, so the effect must depend on it, not on
  // the object identity of the search.
  const href = needsRewrite ? normalizeBoardHref(anchor, { ...search, board: active }) : null;
  const navigateRef = React.useRef(navigate);
  navigateRef.current = navigate;

  React.useEffect(() => {
    if (href === null) return;
    void navigateRef.current({ href, replace: true });
  }, [href]);

  const select = React.useCallback(
    (layout: SpaceLayout) => {
      if (layout === active) return;
      void navigateRef.current({ href: normalizeBoardHref(anchor, { ...search, board: layout }) });
    },
    [active, anchor, search]
  );

  return { offered, active, select };
}
