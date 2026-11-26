// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Compact property trigger (NEWPIDASH-10): the chip the create/edit
// dialog's property row renders for every picker. One small bordered
// button with a leading glyph and truncated text, so all eleven
// properties share a height and treatment. Pickers opt in through
// their `chip` flag; their popups, search and keyboard handling stay
// untouched.
import { Button, Tooltip, cn, type ButtonProps } from "@pidash/kit";
import * as React from "react";

/** Value text the tooltip names when the property has nothing set. */
export const CHIP_EMPTY_VALUE = "None";

/**
 * Tooltip text for a property chip: "Field: Value" plus one short
 * sentence saying what the property is (NEWPIDASH-361). An unset value
 * reads as "None".
 */
export function describeChip(field: string, value: string | null | undefined, sentence: string): string {
  const shown = value === null || value === undefined || value.trim() === "" ? CHIP_EMPTY_VALUE : value;
  return `${field}: ${shown} — ${sentence}`;
}

export interface PropertyChipProps extends Omit<ButtonProps, "size" | "variant" | "children" | "aria-label"> {
  /** Accessible name, e.g. "State: Backlog" or "Assignees". */
  label: string;
  /** Leading glyph (a state dot, an avatar stack); hidden from assistive tech. */
  icon?: React.ReactNode;
  /** Visible text: the value, or the property name when empty. */
  text?: string | undefined;
  /** Muted text treatment for the empty placeholder. */
  placeholder?: boolean;
  /** Borderless treatment (picked assignees show bare avatars). */
  borderless?: boolean;
  /** Extra classes for the text node (an overdue date stays red). */
  textClassName?: string | undefined;
  /** Trailing nodes after the text (a pod name, a wait badge). */
  trailing?: React.ReactNode;
  /**
   * Descriptive hover tooltip ("Field: Value" plus one sentence,
   * NEWPIDASH-361). When set, the chip shows this rich tooltip instead
   * of a native `title`, so callers pass one or the other.
   */
  tooltip?: React.ReactNode;
}

/** One property chip: small button, glyph, truncated text, shared by every picker. */
export function PropertyChip({
  label,
  icon,
  text,
  placeholder = false,
  borderless = false,
  textClassName,
  trailing,
  tooltip,
  className,
  ...rest
}: PropertyChipProps): React.ReactElement {
  const button = (
    <Button
      size="small"
      variant={borderless ? "ghost" : "secondary"}
      aria-label={label}
      className={cn("max-w-48", className)}
      {...rest}
    >
      {icon === undefined || icon === null ? null : (
        <span aria-hidden="true" className="inline-flex shrink-0 items-center">
          {icon}
        </span>
      )}
      {text === undefined || text === "" ? null : (
        <span
          className={cn(
            "min-w-0 flex-1 truncate text-left",
            placeholder ? "text-(--text-muted)" : undefined,
            textClassName
          )}
        >
          {text}
        </span>
      )}
      {trailing}
    </Button>
  );
  if (tooltip === undefined) return button;
  return <Tooltip content={tooltip}>{button}</Tooltip>;
}
