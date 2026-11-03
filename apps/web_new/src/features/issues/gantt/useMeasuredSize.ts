// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Element size measurement for the timeline (ISS-048). Center-on-today
// and the edge trigger both need the strip's real width; a
// ResizeObserver reports it on mount and on every resize. Older engines
// without ResizeObserver fall back to one synchronous read.

import * as React from "react";

export interface MeasuredSize {
  width: number;
  height: number;
}

/** Watch one element's border-box size; absence reports zero. */
export function useMeasuredSize(node: HTMLElement | null): MeasuredSize {
  const [size, setSize] = React.useState<MeasuredSize>({ width: 0, height: 0 });
  React.useEffect(() => {
    if (node === null) return;
    const report = (width: number, height: number): void => {
      setSize((prev) => (prev.width === width && prev.height === height ? prev : { width, height }));
    };
    report(node.clientWidth, node.clientHeight);
    if (typeof globalThis.ResizeObserver !== "function") return;
    const observer = new globalThis.ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (box === undefined) return;
      report(box.width, box.height);
    });
    observer.observe(node);
    return () => {
      observer.disconnect();
    };
  }, [node]);
  return size;
}
