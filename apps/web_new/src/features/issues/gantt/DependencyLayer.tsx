// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dependency layer over the timeline strip (parity/specs/issues.md
// §20/§23, ISS-237). Cloud-only: the community build never mounts it, so
// its flag and its word stay out of the DOM and the oracle sees no
// dependency chrome. The layer draws one arrow per finish-to-start edge at
// the bars' own geometry and exposes a right-edge handle per bar: pressing
// a handle and releasing on another row's bar links the pair.

import * as React from "react";

import { ARROW_MARKER_ID, dependencyPath, type DependencyEdge, type DependencyEnd } from "./dependencies.js";

export interface DependencyLayerProps {
  /** Bars to draw, keyed by row id. */
  ends: ReadonlyMap<string, DependencyEnd>;
  edges: readonly DependencyEdge[];
  /** Pixels covered by the strip, so the layer never clips an arrow. */
  width: number;
  height: number;
  /** Distance from the strip's top to the first lane (the header). */
  top?: number;
  /** Drag-to-link; absent makes the layer pure decoration. */
  onLink?: ((fromId: string, toId: string) => void) | undefined;
  /** Rows a link may end on (bars that are drawn). */
  canLinkTo?: ((toId: string) => boolean) | undefined;
}

interface LinkDrag {
  /** The bar the drag started from, and where its right edge sits. */
  fromId: string;
  originX: number;
  originY: number;
  /** The layer's viewport origin when the drag started. */
  left: number;
  top: number;
  /** Current pointer position, in layer coordinates. */
  x: number;
  y: number;
}

/**
 * Pointer position inside the layer, from a viewport point. Kept out of
 * the component so the arithmetic is testable without a laid-out SVG.
 */
export function layerPoint(
  rect: { left: number; top: number },
  clientX: number,
  clientY: number
): { x: number; y: number } {
  return { x: clientX - rect.left, y: clientY - rect.top };
}

/**
 * The row a released link lands on: the nearest lane ancestor's id, or
 * null when the release missed every lane.
 */
export function laneAt(target: Element | null): string | null {
  return target?.closest("[data-gantt-lane]")?.getAttribute("data-lane-id") ?? null;
}

export function DependencyLayer({
  ends,
  edges,
  width,
  height,
  top = 0,
  onLink,
  canLinkTo,
}: DependencyLayerProps): React.ReactElement | null {
  const [drag, setDrag] = React.useState<LinkDrag | null>(null);
  const layerRef = React.useRef<SVGSVGElement | null>(null);

  React.useEffect(() => {
    if (drag === null) return;
    const onMove = (event: MouseEvent): void => {
      setDrag({ ...drag, ...layerPoint(drag, event.clientX, event.clientY) });
    };
    const onUp = (event: MouseEvent): void => {
      setDrag(null);
      const toId = laneAt(event.target as Element | null);
      if (toId === null || toId === drag.fromId || !(canLinkTo?.(toId) ?? true)) return;
      onLink?.(drag.fromId, toId);
    };
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === "Escape") setDrag(null);
    };
    globalThis.addEventListener("mousemove", onMove);
    globalThis.addEventListener("mouseup", onUp);
    globalThis.addEventListener("keydown", onKey);
    return () => {
      globalThis.removeEventListener("mousemove", onMove);
      globalThis.removeEventListener("mouseup", onUp);
      globalThis.removeEventListener("keydown", onKey);
    };
  }, [canLinkTo, drag, onLink]);

  if (ends.size === 0) return null;

  const paths = edges
    .map((edge) => {
      const from = ends.get(edge.fromId);
      const to = ends.get(edge.toId);
      return from === undefined || to === undefined
        ? null
        : { key: `${edge.fromId}->${edge.toId}`, d: dependencyPath(from, to) };
    })
    .filter((row): row is { key: string; d: string } => row !== null);

  return (
    <svg
      ref={layerRef}
      data-testid="gantt-dependency-layer"
      aria-hidden="true"
      className="pointer-events-none absolute left-0 z-[5]"
      style={{ top }}
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
    >
      <defs>
        <marker id={ARROW_MARKER_ID} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
          <path d="M 0 0 L 8 4 L 0 8 z" fill="var(--text-muted)" />
        </marker>
      </defs>
      {paths.map((row) => (
        <path
          key={row.key}
          d={row.d}
          data-testid="gantt-dependency-arrow"
          fill="none"
          stroke="var(--text-muted)"
          strokeWidth={1.5}
          markerEnd={`url(#${ARROW_MARKER_ID})`}
        />
      ))}
      {drag === null ? null : (
        <line
          data-testid="gantt-dependency-drag-line"
          x1={drag.originX}
          y1={drag.originY}
          x2={drag.x}
          y2={drag.y}
          stroke="var(--accent)"
          strokeDasharray="3 3"
          strokeWidth={1.5}
        />
      )}
      {onLink === undefined
        ? null
        : [...ends.entries()].map(([id, end]) => (
            <circle
              key={id}
              data-testid="gantt-dependency-handle"
              data-source-id={id}
              cx={end.box.left + end.box.width}
              cy={end.laneTop + end.laneHeight / 2}
              r={4}
              className="pointer-events-auto cursor-crosshair"
              fill="var(--bg)"
              stroke="var(--text-muted)"
              strokeWidth={1.5}
              onMouseDown={(event) => {
                event.preventDefault();
                event.stopPropagation();
                const rect = layerRef.current!.getBoundingClientRect();
                setDrag({
                  fromId: id,
                  originX: end.box.left,
                  originY: end.laneTop,
                  left: rect.left,
                  top: rect.top,
                  ...layerPoint(rect, event.clientX, event.clientY),
                });
              }}
            />
          ))}
    </svg>
  );
}
