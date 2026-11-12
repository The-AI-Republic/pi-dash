// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { cn } from "../lib/cn";

export type SpinnerSize = "small" | "medium" | "large";

export interface SpinnerProps {
  size?: SpinnerSize;
  /** Accessible announcement while active. */
  label?: string;
  className?: string;
}

const sizeClasses: Record<SpinnerSize, string> = {
  small: "size-3",
  medium: "size-4",
  large: "size-5",
};

export function Spinner({ size = "medium", label = "Loading", className }: SpinnerProps): React.ReactElement {
  return (
    <span role="status" className={cn("inline-flex", className)}>
      <span
        aria-hidden="true"
        className={cn("animate-spin rounded-full border-2 border-current border-t-transparent", sizeClasses[size])}
      />
      <span className="sr-only">{label}</span>
    </span>
  );
}
