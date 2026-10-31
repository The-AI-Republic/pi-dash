// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Background-fetch gauge for the refresh control (parity/specs/
// notifications.md §7, NTF-013). A cached stream's refetch keeps its data
// while the request runs, so the list's pending flag never turns true;
// this gauge counts started-but-unfinished re-reads instead, which is what
// the refresh control's progress state is. It is not a subscription to
// live data: nothing here re-reads on its own (NTF-028).
import * as React from "react";

export interface BackgroundFetchGauge {
  /** One re-read started; resolves to a finish function. */
  begin: () => () => void;
  /** Re-reads started and not yet finished. */
  count: number;
}

export function useBackgroundFetchGauge(): BackgroundFetchGauge {
  const [count, setCount] = React.useState(0);
  const begin = React.useCallback(() => {
    setCount((current) => current + 1);
    let finished = false;
    return () => {
      if (finished) return;
      finished = true;
      setCount((current) => current - 1);
    };
  }, []);
  return React.useMemo(() => ({ begin, count }), [begin, count]);
}
