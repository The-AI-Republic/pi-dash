// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import * as React from "react";
import { cn } from "../lib/cn";

export type BadgeTone = "neutral" | "accent" | "success" | "warning" | "danger";

export interface BadgeProps {
  tone?: BadgeTone;
  /**
   * Show the status dot. Status badges pair the dot with the label (the dot
   * repeats the text, so the status hues are never small text themselves);
   * plain counts render without it.
   */
  dot?: boolean;
  className?: string;
  children: React.ReactNode;
}

const dotClasses: Record<BadgeTone, string> = {
  neutral: "bg-(--text-muted)",
  accent: "bg-(--accent)",
  success: "bg-(--success)",
  warning: "bg-(--warning)",
  danger: "bg-(--danger)",
};

export function Badge({ tone = "neutral", dot = false, className, children }: BadgeProps): React.ReactElement {
  return (
    <span
      className={cn(
        "text-meta inline-flex items-center gap-(--space-2) rounded-(--radius-full) bg-(--subtle) px-(--space-3) py-px whitespace-nowrap text-(--text)",
        className
      )}
    >
      {dot ? <span aria-hidden="true" className={cn("size-1.5 rounded-full", dotClasses[tone])} /> : null}
      {children}
    </span>
  );
}
