// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hover preview primitive: a small card that opens beside its trigger
// after a short delay and stays open while the pointer crosses into it,
// so the card can be read without the reference disappearing. Anchor it
// to any reference surface (a link, a calendar block, a timeline bar);
// the caller owns the card's content and styling.

import { PreviewCard as BasePreviewCard } from "@base-ui-components/react/preview-card";
import * as React from "react";
import { cn } from "../lib/cn";

/** Default pause before the card shows, long enough to skip pass-through hovers. */
export const PREVIEW_CARD_DELAY_MS = 300;

export interface PreviewCardProps {
  /** The reference the card describes; rendered through Base UI's trigger. */
  trigger: React.ReactElement<Record<string, unknown>>;
  /** Card body. */
  children: React.ReactNode;
  /** Delay before opening in ms. */
  delay?: number | undefined;
  /** Preferred side of the trigger. */
  side?: "top" | "right" | "bottom" | "left" | undefined;
  /** Alignment along the trigger. */
  align?: "start" | "center" | "end" | undefined;
  className?: string | undefined;
}

export function PreviewCard({
  trigger,
  children,
  delay = PREVIEW_CARD_DELAY_MS,
  side = "bottom",
  align = "start",
  className,
}: PreviewCardProps): React.ReactElement {
  return (
    <BasePreviewCard.Root>
      <BasePreviewCard.Trigger delay={delay} render={trigger} />
      <BasePreviewCard.Portal>
        <BasePreviewCard.Positioner side={side} align={align} sideOffset={6}>
          <BasePreviewCard.Popup
            data-testid="preview-card"
            className={cn(
              "shadow-popover text-body w-72 rounded-(--radius-popover) border border-(--border) bg-(--surface) p-(--space-6) text-(--text)",
              className
            )}
          >
            {children}
          </BasePreviewCard.Popup>
        </BasePreviewCard.Positioner>
      </BasePreviewCard.Portal>
    </BasePreviewCard.Root>
  );
}
