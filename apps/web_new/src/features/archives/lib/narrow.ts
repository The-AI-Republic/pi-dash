// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archives back-control visibility (parity/specs/archives.md §1.3,
// ARCH-002). The breadcrumb back control renders only at narrow
// viewports; the desktop header carries the trail without it.
// matchMedia only: no resize listeners, no polling. Without matchMedia
// (SSR, old engines) viewports read as wide.
import * as React from "react";

/** Viewports at or below this width show the breadcrumb back control. */
export const ARCHIVES_BACK_BREAKPOINT_PX = 640;

export function useArchivesBackVisible(): boolean {
  const query = `(max-width: ${ARCHIVES_BACK_BREAKPOINT_PX}px)`;
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
