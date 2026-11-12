// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// SPC-026: a group asks for its next page when its sentinel becomes visible
// inside its own scrolling container (a board column) or the page. The
// caller decides whether a request may start; while one is in flight the
// effect is disabled so the sentinel cannot fire twice.

import * as React from "react";

export interface LoadMoreOnIntersectOptions {
  /** Root the sentinel must be visible within; null means the viewport. */
  root: Element | null;
  /** Whether the caller may start a request right now. */
  enabled: boolean;
  onLoadMore: () => void;
}

/**
 * Attach the returned ref to a sentinel below the group's rows. The observer
 * is rebuilt whenever the root, the enabled flag or the callback identity
 * changes, so a stale in-flight callback can never fire.
 */
export function useLoadMoreOnIntersect({
  root,
  enabled,
  onLoadMore,
}: LoadMoreOnIntersectOptions): React.RefObject<HTMLDivElement | null> {
  const sentinel = React.useRef<HTMLDivElement | null>(null);
  const callbackRef = React.useRef(onLoadMore);
  callbackRef.current = onLoadMore;

  React.useEffect(() => {
    const node = sentinel.current;
    if (!enabled || node === null) return;
    if (typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) callbackRef.current();
      },
      { root, rootMargin: "0px 0px 120px 0px" }
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [enabled, root]);

  return sentinel;
}
