// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared in-palette keyboard flow (shell spec, SHELL-083). Both surfaces —
// the centered modal and the top-bar box — move, activate, clear-then-close
// and step back through this hook so the flow stays identical. The highlight
// always clamps into the row range, so Enter activates exactly the row the
// highlight shows.

import * as React from "react";

export interface PaletteKeyOptions {
  /** Rows available to the highlight; zero disables move and activate. */
  count: number;
  /**
   * Current filter text; drives clear-then-close and the highlight reset.
   * Surface switches need no extra signal: every view remounts on open.
   */
  query: string;
  /** Activate the row at the clamped index. */
  onActivate: (index: number) => void;
  /** Empty the filter without closing (Escape with a query). */
  onClearQuery: () => void;
  /** Close the surface (Escape on an empty query). */
  onDismiss: () => void;
  /**
   * Step back from a sub-page (Backspace on an empty query). Omitted where
   * there is no page to leave; the key then does nothing.
   */
  onStepBack?: (() => void) | undefined;
}

export interface PaletteKeys {
  /** Clamped highlight index, or -1 when there are no rows. */
  activeIndex: number;
  /** Move the highlight directly (pointer hover). Clamped on read. */
  setActiveIndex: (index: number) => void;
  onKeyDown: (event: React.KeyboardEvent) => void;
}

export function usePaletteKeys({
  count,
  query,
  onActivate,
  onClearQuery,
  onDismiss,
  onStepBack,
}: PaletteKeyOptions): PaletteKeys {
  const [highlight, setHighlight] = React.useState(0);

  React.useEffect(() => {
    setHighlight(0);
  }, [query]);

  const activeIndex = count === 0 ? -1 : Math.min(highlight, count - 1);

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (count > 0) setHighlight(Math.min(highlight + 1, count - 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (count > 0) setHighlight(Math.max(highlight - 1, 0));
    } else if (event.key === "Enter") {
      // A focused row or switch activates through its own native click;
      // handling it here too would fire the action twice.
      if (event.target instanceof HTMLButtonElement) return;
      event.preventDefault();
      if (activeIndex >= 0) onActivate(activeIndex);
    } else if (event.key === "Escape") {
      // Always swallowed: the dialog's own dismiss must not fire on the
      // clear press, and the empty press dismisses through the store.
      event.preventDefault();
      event.stopPropagation();
      if (query !== "") onClearQuery();
      else onDismiss();
    } else if (event.key === "Backspace" && query === "" && onStepBack) {
      event.preventDefault();
      onStepBack();
    }
  };

  return { activeIndex, setActiveIndex: setHighlight, onKeyDown };
}
