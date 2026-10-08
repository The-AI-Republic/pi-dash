// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Tooltip as BaseTooltip } from "@base-ui-components/react/tooltip";
import * as React from "react";
import { pickDefined } from "../lib/props";

export interface TooltipProps {
  /** Element the hint anchors to. */
  children: React.ReactElement<Record<string, unknown>>;
  /** Short hint text. */
  content: string;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

export function Tooltip({ children, content, open, onOpenChange }: TooltipProps): React.ReactElement {
  return (
    <BaseTooltip.Root {...pickDefined({ open, onOpenChange })}>
      <BaseTooltip.Trigger render={children} />
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner sideOffset={6}>
          <BaseTooltip.Popup
            role="tooltip"
            className="text-meta shadow-popover max-w-64 rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-2) text-(--text)"
          >
            {content}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}
