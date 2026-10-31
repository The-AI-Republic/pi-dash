// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Binary on/off control (parity/specs/admin.md §12.2: the first slice
// needing a toggle adds it to kit; later slices reuse it). Base UI owns
// the switch semantics and keyboard behavior; kit owns the shape and
// the tokens. Callers name the switch with `label` or the explicit
// ARIA labelling attributes.
import { Switch as BaseSwitch } from "@base-ui-components/react/switch";
import * as React from "react";
import { cn } from "../lib/cn";
import { pickDefined } from "../lib/props";
import { FOCUS_RING } from "../lib/styles";

export interface SwitchProps {
  /** Controlled state. Omit with `defaultChecked` for uncontrolled use. */
  checked?: boolean | undefined;
  /** Initial state for uncontrolled use. */
  defaultChecked?: boolean | undefined;
  /** Fires with the next state on toggle. */
  onCheckedChange?: ((checked: boolean) => void) | undefined;
  /** Accessible-name shorthand; ignored when `aria-label` is set. */
  label?: string | undefined;
  disabled?: boolean | undefined;
  "aria-label"?: string | undefined;
  "aria-labelledby"?: string | undefined;
  "data-testid"?: string | undefined;
  className?: string | undefined;
}

export function Switch({
  checked,
  defaultChecked,
  onCheckedChange,
  label,
  disabled,
  className,
  ...labelled
}: SwitchProps): React.ReactElement {
  return (
    <BaseSwitch.Root
      {...pickDefined({ checked, defaultChecked, disabled })}
      onCheckedChange={(next) => onCheckedChange?.(next)}
      {...pickDefined(labelled)}
      aria-label={labelled["aria-label"] ?? label}
      className={cn(
        "inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border border-(--border) bg-(--subtle) px-0.5 transition-colors duration-(--motion-duration) data-checked:border-transparent data-checked:bg-(--accent) data-disabled:cursor-not-allowed data-disabled:opacity-50",
        FOCUS_RING,
        className
      )}
    >
      <BaseSwitch.Thumb
        aria-hidden="true"
        className="shadow-sm size-3.5 rounded-full bg-white transition-transform duration-(--motion-duration) data-checked:translate-x-4"
      />
    </BaseSwitch.Root>
  );
}
