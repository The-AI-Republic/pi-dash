// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
