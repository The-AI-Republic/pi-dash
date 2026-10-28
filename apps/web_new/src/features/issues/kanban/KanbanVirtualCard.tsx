// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Virtualized board card shell (parity/specs/issues.md §5, ISS-043).
// Every loaded row mounts a shell carrying the card markers, so column
// reads and drop resolution see the whole column; the card content
// itself mounts while vertically near the viewport and unmounts past
// the vertical overscan, leaving a height-estimated placeholder behind.
// Horizontal distance never unmounts: every column mounts its window
// no matter how far the board scrolled sideways (ISS-039).
//
// The window observes with the board's own scroller as its root. The
// browser clips an observer without an explicit root to the nearest
// scrolling ancestor, so an implicit root would only ever reach the
// columns inside the board's scrollport and strand every far column as
// a content-less shell no matter how wide the margins are. Rooting at
// the board keeps the horizontal reach meaningful, and the browser
// still accounts for the column body's clip, so tall columns keep
// windowing vertically (ISS-043).
//
// Without an observer (tests, SSR) shells render content immediately.
// Both states pin shrink-0: the column body is a flex column and would
// otherwise crush its rows to nothing, hiding every page past the first
// (NEWFRONT-968).

import * as React from "react";

/** Placeholder height standing in for an unmounted card. */
export const VIRTUAL_CARD_HEIGHT_PX = 120;
/** Distance past the viewport's top and bottom edge that still mounts content. */
export const VIRTUAL_OVERSCAN_PX = 800;
/**
 * Horizontal reach of the card window. Deliberately beyond any plausible
 * board width, so a lone card in a far column renders its content while
 * tall columns still window vertically (ISS-039 vs ISS-043).
 */
export const VIRTUAL_CROSS_OVERSCAN_PX = 1_000_000;

/** Observer margins for the card window: vertical overscan, unbounded horizontal reach. */
export function virtualCardRootMargin(overscan = VIRTUAL_OVERSCAN_PX): string {
  return `${overscan}px ${VIRTUAL_CROSS_OVERSCAN_PX}px ${overscan}px ${VIRTUAL_CROSS_OVERSCAN_PX}px`;
}

/**
 * Whether the node sits within the card window's vertical overscan.
 * `root` names the board scroller the window measures against, so the
 * browser measures against the board rather than clipping the implicit
 * viewport root to the board's scrollport. Until the board hands its
 * scroller over the window falls back to the implicit root.
 */
export function useNearViewport<T extends HTMLElement>(
  overscan = VIRTUAL_OVERSCAN_PX,
  root: HTMLElement | null = null
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
      { root, rootMargin: virtualCardRootMargin(overscan) }
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [overscan, root]);
  return { ref, near };
}

export interface KanbanVirtualCardProps {
  issueId: string;
  groupKey: string;
  laneKey: string | null;
  /** Mount content regardless of the viewport (drag source, dropped card). */
  forceContent: boolean;
  /** The board scroller the card window measures against. */
  root?: HTMLElement | null;
  children: React.ReactNode;
}

export function KanbanVirtualCard({
  issueId,
  groupKey,
  laneKey,
  forceContent,
  root = null,
  children,
}: KanbanVirtualCardProps): React.ReactElement {
  const { ref, near } = useNearViewport<HTMLDivElement>(VIRTUAL_OVERSCAN_PX, root);
  if (near || forceContent) {
    return (
      <div ref={ref} className="shrink-0">
        {children}
      </div>
    );
  }
  return (
    <div
      ref={ref}
      className="shrink-0"
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
