// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hover preview primitive: a small card that opens beside its trigger
// after a short delay and stays open while the pointer crosses into it,
// so the card can be read without the reference disappearing. Anchor it
// to any reference surface (a link, a calendar block, a timeline bar);
// the caller owns the card's content and styling.

import * as React from "react";
import { cn } from "../lib/cn";

/** Default pause before the card shows, long enough to skip pass-through hovers. */
export const PREVIEW_CARD_DELAY_MS = 300;

/** Gap between the reference and the card, in px. */
const CARD_GAP_PX = 6;

/** Time a pointer has to reach the card before the reference's leave closes it. */
const LEAVE_GRACE_MS = 100;

export interface PreviewCardProps {
  /** The reference the card describes; cloned to carry the hover handlers. */
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
  const [anchor, setAnchor] = React.useState<DOMRect | null>(null);
  const timer = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancel = React.useCallback(() => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
  }, []);

  React.useEffect(() => cancel, [cancel]);

  // Opening waits out the delay so a pointer crossing the reference on its
  // way somewhere else never flashes a card.
  const open = (element: Element) => {
    cancel();
    if (delay <= 0) {
      setAnchor(element.getBoundingClientRect());
      return;
    }
    timer.current = setTimeout(() => {
      timer.current = null;
      setAnchor(element.getBoundingClientRect());
    }, delay);
  };

  // The card sits beside the reference, so a pointer crossing the gap is
  // still "on" the pair. Leaving the reference waits a beat before closing:
  // if the pointer landed on the card, its enter cancels the close.
  const leave = () => {
    cancel();
    timer.current = setTimeout(() => {
      timer.current = null;
      setAnchor(null);
    }, LEAVE_GRACE_MS);
  };

  // Choosing the card takes focus off the reference; that blur must not
  // tear the card down under the pointer.
  const blur = () => {
    if (timer.current === null) setAnchor(null);
  };

  const reference = React.cloneElement(trigger, {
    onPointerEnter: (event: React.PointerEvent<Element>) => {
      open(event.currentTarget);
      (trigger.props.onPointerEnter as ((event: React.PointerEvent<Element>) => void) | undefined)?.(event);
    },
    onPointerLeave: (event: React.PointerEvent<Element>) => {
      leave();
      (trigger.props.onPointerLeave as ((event: React.PointerEvent<Element>) => void) | undefined)?.(event);
    },
    onFocus: (event: React.FocusEvent<Element>) => {
      open(event.currentTarget);
      (trigger.props.onFocus as ((event: React.FocusEvent<Element>) => void) | undefined)?.(event);
    },
    onBlur: (event: React.FocusEvent<Element>) => {
      blur();
      (trigger.props.onBlur as ((event: React.FocusEvent<Element>) => void) | undefined)?.(event);
    },
  });

  return (
    <>
      {reference}
      {anchor === null ? null : (
        <div
          data-testid="preview-card"
          className={cn(
            "shadow-popover text-body fixed z-50 w-72 rounded-(--radius-popover) border border-(--border) bg-(--surface) p-(--space-6) text-(--text)",
            className
          )}
          style={cardPosition(anchor, side, align)}
          onPointerEnter={cancel}
          onPointerLeave={leave}
        >
          {children}
        </div>
      )}
    </>
  );
}

/** The card's fixed position around its reference's viewport rectangle. */
function cardPosition(
  anchor: DOMRect,
  side: NonNullable<PreviewCardProps["side"]>,
  align: NonNullable<PreviewCardProps["align"]>
): React.CSSProperties {
  if (side === "top" || side === "bottom") {
    const top = side === "top" ? anchor.top - CARD_GAP_PX : anchor.bottom + CARD_GAP_PX;
    const left = align === "end" ? anchor.right : align === "center" ? (anchor.left + anchor.right) / 2 : anchor.left;
    return {
      top,
      left,
      transform: `translate(${align === "end" ? "-100%" : align === "center" ? "-50%" : "0"}, ${
        side === "top" ? "-100%" : "0"
      })`,
    };
  }
  const left = side === "left" ? anchor.left - CARD_GAP_PX : anchor.right + CARD_GAP_PX;
  const top = align === "end" ? anchor.bottom : align === "center" ? (anchor.top + anchor.bottom) / 2 : anchor.top;
  return {
    top,
    left,
    transform: `translate(${side === "left" ? "-100%" : "0"}, ${
      align === "end" ? "-100%" : align === "center" ? "-50%" : "0"
    })`,
  };
}
