// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stream restart on control changes (parity/specs/notifications.md §7, §8,
// §11; NTF-015, NTF-016, NTF-028). A tab, filter or mode change must
// start the stream over from its first page rather than serve whatever
// the previous visit cached, so the current stream re-reads after every
// signature change. The first render does not re-read: entry already
// fetched. No timer or subscription lives here — this only reacts to the
// user's own control changes (NTF-028).
import * as React from "react";

export function useStreamReset(signature: string, refetch: () => Promise<unknown>): void {
  const first = React.useRef(true);
  const latest = React.useRef(refetch);
  latest.current = refetch;
  React.useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    void latest.current();
  }, [signature]);
}
