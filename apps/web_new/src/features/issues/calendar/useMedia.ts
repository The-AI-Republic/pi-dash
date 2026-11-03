// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Viewport helpers for the calendar header and drag gates. The compact
// header renders one control set, so callers must not mount both the
// desktop and mobile variants at once.

import * as React from "react";

function matches(query: string): boolean {
  if (typeof globalThis.window === "undefined") return false;
  if (typeof globalThis.window.matchMedia !== "function") return false;
  return globalThis.window.matchMedia(query).matches;
}

/** True below the md breakpoint. */
export function useNarrowViewport(): boolean {
  const [narrow, setNarrow] = React.useState(() => matches("(max-width: 767px)"));
  React.useEffect(() => {
    if (typeof globalThis.window === "undefined" || typeof globalThis.window.matchMedia !== "function") return;
    const query = globalThis.window.matchMedia("(max-width: 767px)");
    const update = (): void => setNarrow(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  return narrow;
}

/** True for touch-primary devices (phones and tablets). */
export function useTouchInput(): boolean {
  return (
    matches("(pointer: coarse)") ||
    (typeof globalThis.navigator !== "undefined" && globalThis.navigator.maxTouchPoints > 0)
  );
}
