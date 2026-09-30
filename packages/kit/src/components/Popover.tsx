// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { Popover as BasePopover } from "@base-ui-components/react/popover";
import * as React from "react";
import { cn } from "../lib/cn";
import { pickDefined } from "../lib/props";

export interface PopoverProps {
  /** Element rendered as the popover trigger. */
  trigger: React.ReactElement<Record<string, unknown>>;
  title?: string | undefined;
  description?: string | undefined;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  className?: string | undefined;
  children: React.ReactNode;
}

export function Popover({
  trigger,
  title,
  description,
  open,
  onOpenChange,
  className,
  children,
}: PopoverProps): React.ReactElement {
  return (
    <BasePopover.Root {...pickDefined({ open, onOpenChange })}>
      <BasePopover.Trigger render={trigger} />
      <BasePopover.Portal>
        <BasePopover.Positioner sideOffset={6}>
          <BasePopover.Popup
            className={cn(
              "shadow-popover w-72 rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-8)",
              className
            )}
          >
            {title ? (
              <BasePopover.Title className="text-emphasis font-medium text-(--text)">{title}</BasePopover.Title>
            ) : null}
            {description ? (
              <BasePopover.Description className="text-body mt-(--space-2) text-(--text-muted)">
                {description}
              </BasePopover.Description>
            ) : null}
            <div className={title || description ? "mt-(--space-6)" : undefined}>{children}</div>
          </BasePopover.Popup>
        </BasePopover.Positioner>
      </BasePopover.Portal>
    </BasePopover.Root>
  );
}
