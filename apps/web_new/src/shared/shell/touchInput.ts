// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Touch-primary input signal (parity/specs/cycles.md §21). A coarse
// pointer or a touch-capable device reveals row actions inline even
// when the viewport is wide, so actions never depend on hover. Like the
// narrow-viewport signal this reads matchMedia only; hosts without it
// (SSR, older engines) report no touch input.
import * as React from "react";

/** Devices whose primary input is touch, independent of viewport width. */
export function useTouchInput(): boolean {
  const query = "(pointer: coarse)";
  const read = (): boolean => {
    if (typeof globalThis.matchMedia !== "function") return false;
    if (globalThis.matchMedia(query).matches) return true;
    return typeof globalThis.navigator !== "undefined" && globalThis.navigator.maxTouchPoints > 0;
  };
  const [touch, setTouch] = React.useState<boolean>(read);
  React.useEffect(() => {
    if (typeof globalThis.matchMedia !== "function") return;
    const list = globalThis.matchMedia(query);
    const update = (): void => setTouch(read());
    update();
    list.addEventListener("change", update);
    return () => {
      list.removeEventListener("change", update);
    };
  }, []);
  return touch;
}
