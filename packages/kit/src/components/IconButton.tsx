// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { cn } from "../lib/cn";
import { FOCUS_RING } from "../lib/styles";
import type { ButtonSize, ButtonVariant } from "./Button";

export interface IconButtonProps extends Omit<
  React.ButtonHTMLAttributes<HTMLButtonElement>,
  "aria-label" | "children"
> {
  /** Accessible name; icon-only buttons expose nothing else to name them. */
  label: string;
  variant?: ButtonVariant;
  size?: ButtonSize;
  children: React.ReactNode;
}

const baseClasses =
  "inline-flex cursor-pointer items-center justify-center rounded-(--radius-control) transition-colors duration-(--motion-duration) disabled:cursor-not-allowed disabled:opacity-50";

const sizeClasses: Record<ButtonSize, string> = {
  small: "size-6",
  medium: "size-(--control-height)",
  large: "size-(--control-height-large)",
};

const variantClasses: Record<ButtonVariant, string> = {
  primary: "bg-(--accent) text-(--on-accent) hover:bg-(--accent-hover)",
  secondary: "border border-(--border) bg-(--bg) text-(--text) hover:bg-(--subtle)",
  ghost: "bg-transparent text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)",
  danger: "border border-(--danger) bg-transparent text-(--danger) hover:bg-(--subtle)",
};

export function IconButton({
  label,
  variant = "ghost",
  size = "medium",
  className,
  children,
  ...rest
}: IconButtonProps): React.ReactElement {
  return (
    <button
      type="button"
      aria-label={label}
      className={cn(baseClasses, sizeClasses[size], variantClasses[variant], FOCUS_RING, className)}
      {...rest}
    >
      {children}
    </button>
  );
}
