// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { cn } from "../lib/cn";
import { FOCUS_RING } from "../lib/styles";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "small" | "medium" | "large";

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Shows a spinner and blocks re-activation while true. */
  loading?: boolean;
}

const baseClasses =
  "inline-flex cursor-pointer items-center justify-center gap-(--space-2) rounded-(--radius-control) font-medium whitespace-nowrap transition-colors duration-(--motion-duration) disabled:cursor-not-allowed disabled:opacity-50";

const sizeClasses: Record<ButtonSize, string> = {
  small: "h-6 px-(--space-3) text-meta",
  medium: "h-(--control-height) px-(--space-4) text-body",
  large: "h-(--control-height-large) px-(--space-6) text-emphasis",
};

const variantClasses: Record<ButtonVariant, string> = {
  primary: "bg-(--accent) text-(--on-accent) hover:bg-(--accent-hover)",
  secondary: "border border-(--border) bg-(--bg) text-(--text) hover:bg-(--subtle)",
  ghost: "bg-transparent text-(--text) hover:bg-(--subtle)",
  // Outline style: the light-theme danger hue sits below AA for filled
  // labels (see tokens.test.ts), so danger actions pair the red text with
  // explicit confirmation copy instead of a filled red button.
  danger: "border border-(--danger) bg-transparent text-(--danger) hover:bg-(--subtle)",
};

export function Button({
  variant = "secondary",
  size = "medium",
  loading = false,
  disabled,
  className,
  children,
  ...rest
}: ButtonProps): React.ReactElement {
  return (
    <button
      type="button"
      disabled={disabled ?? loading}
      aria-busy={loading || undefined}
      className={cn(baseClasses, sizeClasses[size], variantClasses[variant], FOCUS_RING, className)}
      {...rest}
    >
      {loading ? (
        <span
          aria-hidden="true"
          className="size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
        />
      ) : null}
      {children}
    </button>
  );
}
