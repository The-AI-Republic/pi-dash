// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { useVirtualizer } from "@tanstack/react-virtual";
import * as React from "react";
import { cn } from "../lib/cn";

export interface VirtualListProps<T> {
  items: T[];
  /** Fixed row height, or per-row estimate for variable rows. */
  estimateSize: number | ((index: number) => number);
  /** Stable key per row; used for row identity across scrolls. */
  getKey: (item: T, index: number) => string | number;
  renderRow: (item: T, index: number) => React.ReactNode;
  /** Rows rendered past each viewport edge. */
  overscan?: number;
  /** Accessible name for the scroll region. */
  label: string;
  className?: string;
}

const containerClasses = "relative overflow-auto";

export function VirtualList<T>({
  items,
  estimateSize,
  getKey,
  renderRow,
  overscan = 4,
  label,
  className,
}: VirtualListProps<T>): React.ReactElement {
  const parentRef = React.useRef<HTMLDivElement>(null);
  const estimate = React.useCallback(
    (index: number) => (typeof estimateSize === "number" ? estimateSize : estimateSize(index)),
    [estimateSize]
  );
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: estimate,
    overscan,
  });
  const virtualItems = virtualizer.getVirtualItems();
  return (
    <div ref={parentRef} role="region" aria-label={label} className={cn(containerClasses, className)}>
      <div style={{ height: virtualizer.getTotalSize(), position: "relative", width: "100%" }}>
        {virtualItems.map((virtualRow) => {
          const item = items[virtualRow.index];
          if (item === undefined) return null;
          return (
            <div
              key={getKey(item, virtualRow.index)}
              data-index={virtualRow.index}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                transform: `translateY(${virtualRow.start}px)`,
              }}
            >
              {renderRow(item, virtualRow.index)}
            </div>
          );
        })}
      </div>
    </div>
  );
}
