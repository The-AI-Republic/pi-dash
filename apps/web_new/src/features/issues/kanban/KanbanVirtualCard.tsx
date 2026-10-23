// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Virtualized board card shell (parity/specs/issues.md §5, ISS-043).
// Every loaded row mounts a shell carrying the card markers, so column
// reads and drop resolution see the whole column; the card content
// itself mounts only near the viewport and unmounts past the overscan,
// leaving a height-estimated placeholder behind. Without an observer
// (tests, SSR) shells render content immediately.

import * as React from "react";

/** Placeholder height standing in for an unmounted card. */
export const VIRTUAL_CARD_HEIGHT_PX = 120;
/** Distance past the viewport edge that still mounts content. */
export const VIRTUAL_OVERSCAN_PX = 800;

/** Whether the node sits within the overscan of the viewport. */
export function useNearViewport<T extends HTMLElement>(
  overscan = VIRTUAL_OVERSCAN_PX
): {
  ref: React.RefObject<T | null>;
  near: boolean;
} {
  const ref = React.useRef<T | null>(null);
  const [near, setNear] = React.useState(false);
  React.useEffect(() => {
    const node = ref.current;
    if (node === null || typeof IntersectionObserver === "undefined") {
      setNear(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        setNear(entries.some((entry) => entry.isIntersecting));
      },
      { rootMargin: `${overscan}px` }
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [overscan]);
  return { ref, near };
}

export interface KanbanVirtualCardProps {
  issueId: string;
  groupKey: string;
  laneKey: string | null;
  /** Mount content regardless of the viewport (drag source, dropped card). */
  forceContent: boolean;
  children: React.ReactNode;
}

export function KanbanVirtualCard({
  issueId,
  groupKey,
  laneKey,
  forceContent,
  children,
}: KanbanVirtualCardProps): React.ReactElement {
  const { ref, near } = useNearViewport<HTMLDivElement>();
  if (near || forceContent) {
    return <div ref={ref}>{children}</div>;
  }
  return (
    <div
      ref={ref}
      data-testid="kanban-card"
      data-issue-id={issueId}
      data-group-key={groupKey}
      data-lane-key={laneKey ?? ""}
      data-virtualized="true"
      aria-hidden="true"
      style={{ height: VIRTUAL_CARD_HEIGHT_PX }}
    />
  );
}
