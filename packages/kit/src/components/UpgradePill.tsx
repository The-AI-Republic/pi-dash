// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Upgrade pill (SHELL-101, from parity/specs/shell.md §5). A small marker
// beside paywalled destinations: sidebar rows, menus and page headers mount
// it in either size. It is plain text, never a control, so it cannot
// swallow clicks or change where its row navigates.
import * as React from "react";
import { cn } from "../lib/cn";

export type UpgradePillSize = "small" | "medium";

export interface UpgradePillProps {
  size?: UpgradePillSize;
  className?: string;
}

const sizeClasses: Record<UpgradePillSize, string> = {
  small: "text-caption px-(--space-2)",
  medium: "text-meta px-(--space-3)",
};

export function UpgradePill({ size = "small", className }: UpgradePillProps): React.ReactElement {
  return (
    <span
      data-testid="upgrade-pill"
      className={cn(
        "inline-flex w-fit items-center rounded-(--radius-full) bg-(--accent-subtle) py-px font-medium whitespace-nowrap text-(--accent)",
        sizeClasses[size],
        className
      )}
    >
      Pro
    </span>
  );
}
