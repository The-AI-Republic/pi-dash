// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
