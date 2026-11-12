// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Narrow-viewport signal (SHELL-069, from parity/specs/shell.md §6). Below
// the breakpoint an open sidebar floats above content and collapses on
// outside interaction; this hook reports which side of the line the
// viewport sits on. matchMedia only: no resize listeners, no polling.
// Without matchMedia (SSR, old engines) viewports read as wide.
import * as React from "react";

/** Viewports narrower than this collapse and float the sidebar. */
export const NARROW_VIEWPORT_BREAKPOINT_PX = 768;

export function useNarrowViewport(breakpoint: number = NARROW_VIEWPORT_BREAKPOINT_PX): boolean {
  const query = `(max-width: ${breakpoint - 1}px)`;
  const [narrow, setNarrow] = React.useState<boolean>(() =>
    typeof globalThis.matchMedia === "function" ? globalThis.matchMedia(query).matches : false
  );
  React.useEffect(() => {
    if (typeof globalThis.matchMedia !== "function") return;
    const list = globalThis.matchMedia(query);
    setNarrow(list.matches);
    const onChange = (event: MediaQueryListEvent): void => {
      setNarrow(event.matches);
    };
    list.addEventListener("change", onChange);
    return () => {
      list.removeEventListener("change", onChange);
    };
  }, [query]);
  return narrow;
}
