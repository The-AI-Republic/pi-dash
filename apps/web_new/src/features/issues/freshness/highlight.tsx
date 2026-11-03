// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Explicit row highlight (parity/specs/issues.md §1, ISS-006). Drops
// highlight the landing row and scroll it into view; the highlight
// clears on the next pointer press outside the row. Fresh issues do not
// need this state — rows derive their highlight from freshness plus the
// id held here (see isRowHighlighted).

import * as React from "react";

import { rowDomId } from "./fresh.js";

export interface HighlightOptions {
  /** Scroll the row into view after highlighting (drops pass true). */
  scroll?: boolean;
}

export interface HighlightHandle {
  highlightedId: string | null;
  highlight: (issueId: string, options?: HighlightOptions) => void;
  clearHighlight: () => void;
}

const HighlightContext = React.createContext<HighlightHandle | null>(null);

function scrollRowIntoView(issueId: string): void {
  document.getElementById(rowDomId(issueId))?.scrollIntoView?.({ block: "nearest" });
}

export function HighlightProvider({ children }: { children: React.ReactNode }): React.ReactElement {
  const [highlightedId, setHighlightedId] = React.useState<string | null>(null);
  const clearHighlight = React.useCallback(() => setHighlightedId(null), []);
  const highlight = React.useCallback((issueId: string, options?: HighlightOptions) => {
    setHighlightedId(issueId);
    if (options?.scroll === true) scrollRowIntoView(issueId);
  }, []);
  React.useEffect(() => {
    if (highlightedId === null) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (document.getElementById(rowDomId(highlightedId))?.contains(event.target as Node | null) === true) return;
      setHighlightedId(null);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [highlightedId]);
  const value = React.useMemo<HighlightHandle>(
    () => ({ highlightedId, highlight, clearHighlight }),
    [highlightedId, highlight, clearHighlight]
  );
  return <HighlightContext.Provider value={value}>{children}</HighlightContext.Provider>;
}

/** Read the highlight handle; every layout root renders under the provider. */
export function useHighlight(): HighlightHandle {
  const handle = React.useContext(HighlightContext);
  if (handle === null) throw new Error("useHighlight must be used under HighlightProvider.");
  return handle;
}
