// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { cn } from "../lib/cn";

export interface SkeletonProps {
  /** Width of the placeholder (any CSS length). */
  width?: string;
  /** Height of the placeholder (any CSS length). */
  height?: string;
  /** Fully round (avatar/dot shapes) instead of control-radius. */
  round?: boolean;
  className?: string;
}

export function Skeleton({
  width = "100%",
  height = "12px",
  round = false,
  className,
}: SkeletonProps): React.ReactElement {
  return (
    <span
      aria-hidden="true"
      style={{ width, height }}
      className={cn(
        "animate-shimmer block bg-[linear-gradient(90deg,var(--subtle)_25%,var(--border)_50%,var(--subtle)_75%)] bg-[length:200%_100%]",
        round ? "rounded-full" : "rounded-(--radius-control)",
        className
      )}
    />
  );
}
