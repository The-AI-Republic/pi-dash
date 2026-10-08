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
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  /** When true, Escape and the backdrop no longer dismiss. */
  nonDismissable?: boolean | undefined;
  className?: string | undefined;
  children: React.ReactNode;
}

export function Dialog({
  trigger,
  title,
  description,
  actions,
  open,
  onOpenChange,
  nonDismissable = false,
  className,
  children,
}: DialogProps): React.ReactElement {
  return (
    <BaseDialog.Root {...pickDefined({ open, onOpenChange })}>
      {trigger ? <BaseDialog.Trigger render={trigger} /> : null}
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className="fixed inset-0 bg-black/40 transition-opacity duration-(--motion-duration)" />
        <BaseDialog.Popup
          className={cn(
            "shadow-dialog fixed top-1/2 left-1/2 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-(--radius-dialog) border border-(--border) bg-(--bg) p-(--space-12)",
            className
          )}
        >
          <div className="flex items-start justify-between gap-(--space-4)">
            <div className="flex flex-col gap-(--space-2)">
              <BaseDialog.Title className="text-h3 text-(--text)">{title}</BaseDialog.Title>
              {description ? (
                <BaseDialog.Description className="text-body text-(--text-muted)">{description}</BaseDialog.Description>
              ) : null}
            </div>
            {nonDismissable ? null : (
              <BaseDialog.Close
                render={
                  <IconButton label="Close dialog">
                    <X size={16} strokeWidth={1.5} />
                  </IconButton>
                }
              />
            )}
          </div>
          <div className="mt-(--space-8)">{children}</div>
          {actions ? <div className="mt-(--space-12) flex justify-end gap-(--space-4)">{actions}</div> : null}
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}
