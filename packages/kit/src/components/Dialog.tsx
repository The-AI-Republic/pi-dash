// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Dialog as BaseDialog } from "@base-ui-components/react/dialog";
import { X } from "lucide-react";
import * as React from "react";
import { cn } from "../lib/cn";
import { pickDefined } from "../lib/props";
import { IconButton } from "./IconButton";

export interface DialogProps {
  /** Element rendered as the dialog trigger. Omit for controlled use. */
  trigger?: React.ReactElement<Record<string, unknown>> | undefined;
  title: string;
  description?: string | undefined;
  /** Footer actions (usually Buttons); rendered right-aligned. */
  actions?: React.ReactNode | undefined;
  /**
   * Header controls placed before the close button: a panel toggle, a
   * scope summary, and the like. They sit in the title row so a page-level
   * panel keeps one action lane.
   */
  headerActions?: React.ReactNode | undefined;
  /**
   * Where the panel sits. `center` is the usual modal card; `right` docks
   * the panel against the right edge at three-quarter width and grows to
   * the full viewport width when `wide` is set. The caller owns the
   * breakpoint that exposes `wide`; the placement itself is plain CSS.
   */
  placement?: "center" | "right" | undefined;
  /** Full-width right panel; ignored while `placement` is `center`. */
  wide?: boolean | undefined;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean, details: { reason: string }) => void) | undefined;
  /**
   * When true, Escape no longer dismisses while the backdrop and the
   * close button still do (the product-updates dialog). Only meaningful
   * with a controlled open: an uncontrolled dialog closes on its own.
   */
  ignoreEscapeKey?: boolean | undefined;
  /** When true, Escape and the backdrop no longer dismiss. */
  nonDismissable?: boolean | undefined;
  /**
   * When false, the header's close control is left out so the dialog can
   * order its own first action (callers still provide a Close control in
   * the body or footer, or combine it with nonDismissable).
   */
  closeButton?: boolean | undefined;
  className?: string | undefined;
  /** Body wrapper classes; the default spacing sits below the title block. */
  bodyClassName?: string | undefined;
  /** Popup classes for `right` panels outside the width rules, e.g. max height. */
  popupClassName?: string | undefined;
  /**
   * Control to focus when the dialog opens. Without it Base UI focuses the
   * first tabbable element, which is the header close button. Pass `false`
   * to take over focusing entirely: Base UI moves no focus and the caller
   * focuses its own control (for dialogs whose first control mounts after
   * data resolves, where the open-time pass would reclaim the container).
   */
  initialFocus?: React.RefObject<HTMLElement | null> | false | undefined;
  children: React.ReactNode;
}

export function Dialog({
  trigger,
  title,
  description,
  actions,
  headerActions,
  placement = "center",
  wide = false,
  open,
  onOpenChange,
  ignoreEscapeKey = false,
  nonDismissable = false,
  closeButton = true,
  className,
  bodyClassName = "mt-(--space-8)",
  popupClassName,
  initialFocus,
  children,
}: DialogProps): React.ReactElement {
  const handleOpenChange = (next: boolean, details: { reason: string }): void => {
    if (!next && nonDismissable) return;
    if (!next && ignoreEscapeKey && details.reason === "escape-key") return;
    onOpenChange?.(next, details);
  };
  // A centred card is capped to the viewport with its body scrolling, so a
  // tall form keeps its tab strip, fields and footer actions reachable.
  // The body's basis stays content-sized (`flex-auto`): the card's height is
  // automatic up to the cap, unlike the docked panel's definite full height.
  const centered = [
    "top-1/2 left-1/2 flex max-h-[calc(100dvh-2rem)] w-full max-w-md flex-col -translate-x-1/2 -translate-y-1/2",
    "rounded-(--radius-dialog)",
  ];
  // A right panel keeps the dialog's fieldset role and focus trap but
  // changes the frame: docked to the right edge, three quarters wide, and
  // full width when the caller opens it up. Width animates so the toggle
  // reads as a resize rather than a jump.
  const docked = [
    "top-0 right-0 h-full max-h-full",
    wide ? "w-full" : "w-3/4",
    "transition-[width] duration-(--motion-duration)",
  ];
  const frame = placement === "right" ? ["flex flex-col", ...docked, popupClassName] : [...centered, popupClassName];
  return (
    <BaseDialog.Root {...pickDefined({ open })} onOpenChange={handleOpenChange}>
      {trigger ? <BaseDialog.Trigger render={trigger} /> : null}
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className="fixed inset-0 bg-black/40 transition-opacity duration-(--motion-duration)" />
        <BaseDialog.Popup
          {...pickDefined({ initialFocus })}
          className={cn("shadow-dialog fixed border border-(--border) bg-(--bg) p-(--space-12)", ...frame, className)}
        >
          <div className="flex shrink-0 items-start justify-between gap-(--space-4)">
            <div className="flex flex-col gap-(--space-2)">
              <BaseDialog.Title className="text-h4-medium text-(--text)">{title}</BaseDialog.Title>
              {description ? (
                <BaseDialog.Description className="text-body text-(--text-muted)">{description}</BaseDialog.Description>
              ) : null}
            </div>
            <div className="flex shrink-0 items-center gap-(--space-2)">
              {headerActions}
              {nonDismissable || !closeButton ? null : (
                <BaseDialog.Close
                  render={
                    <IconButton label="Close dialog">
                      <X size={16} strokeWidth={1.5} />
                    </IconButton>
                  }
                />
              )}
            </div>
          </div>
          {placement === "right" ? (
            // The document role marks the scrollable panel content inside
            // the dialog, which is what a region keyboard users move through
            // and what automation measures when the panel resizes.
            <div role="document" className={cn(bodyClassName, "min-h-0 flex-1 basis-0 overflow-y-auto")}>
              {children}
            </div>
          ) : (
            <div role="document" className={cn(bodyClassName, "min-h-0 flex-auto overflow-y-auto")}>
              {children}
            </div>
          )}
          {actions ? <div className="mt-(--space-12) flex shrink-0 justify-end gap-(--space-4)">{actions}</div> : null}
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}
