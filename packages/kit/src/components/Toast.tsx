// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Toasts: a stack of short confirmations. A notice carries a title, an
// optional body and up to a few inline actions: a labelled button, an
// anchor that opens in a new tab (the "view" affordance after a write),
// or a copy action that swaps to its confirmation for a few seconds so
// the user sees the clipboard write land. The confirmation is client
// state only; it never changes what was copied.
import { Toast as BaseToast } from "@base-ui-components/react/toast";
import { X } from "lucide-react";
import * as React from "react";
import { IconButton } from "./IconButton";

export type ToastHostManager = ReturnType<typeof BaseToast.createToastManager>;

type ToastItem = ReturnType<typeof BaseToast.useToastManager>["toasts"][number];

/** How long a copy action shows its confirmation. */
export const TOAST_COPIED_RESET_MS = 3000;

interface ToastActionData {
  actionLabel?: string | undefined;
  onAction?: (() => void) | undefined;
  actions?: readonly ToastAction[] | undefined;
}

/** One inline action row on a toast. */
export interface ToastAction {
  /** Visible label, e.g. "View work item" or "Copy link". */
  label: string;
  /** Runs when the action is chosen (for a link, when it is followed). */
  run?: (() => void) | undefined;
  /** Render as an anchor opened in a new tab at this address. */
  href?: string | undefined;
}

export function createKitToastManager(): ToastHostManager {
  return BaseToast.createToastManager();
}

export interface ShowToastOptions {
  title: string;
  description?: string;
  actionLabel?: string;
  onAction?: () => void;
  /** Inline actions rendered under the body, in order. */
  actions?: readonly ToastAction[];
  /** Auto-dismiss delay in ms; 0 keeps the toast until dismissed. */
  timeout?: number;
}

export function showToast(manager: ToastHostManager, options: ShowToastOptions): string {
  return manager.add<ToastActionData>({
    title: options.title,
    ...(options.description === undefined ? {} : { description: options.description }),
    ...(options.timeout === undefined ? {} : { timeout: options.timeout }),
    data: {
      ...(options.actionLabel === undefined ? {} : { actionLabel: options.actionLabel }),
      ...(options.onAction === undefined ? {} : { onAction: options.onAction }),
      ...(options.actions === undefined ? {} : { actions: options.actions }),
    },
  });
}

function actionOf(toast: ToastItem): ToastActionData {
  return (toast.data as ToastActionData | undefined) ?? {};
}

const actionClasses =
  "inline-flex cursor-pointer items-center rounded-(--radius-control) px-(--space-2) py-px font-medium text-(--accent) hover:bg-(--subtle) hover:text-(--accent-hover) focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-(--focus-ring-color) focus-visible:outline-solid";

/** One action row; a copy action keeps a short "Copied!" confirmation. */
function ToastActionItem({ action, onChosen }: { action: ToastAction; onChosen: () => void }): React.ReactElement {
  const [copied, setCopied] = React.useState(false);
  const timer = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  React.useEffect(
    () => () => {
      if (timer.current !== null) clearTimeout(timer.current);
    },
    []
  );
  // Copy actions are the ones with a run callback and no address: the
  // clipboard write is the side effect, and its result is reported here.
  if (action.href !== undefined) {
    return (
      <a
        href={action.href}
        target="_blank"
        rel="noopener noreferrer"
        className={actionClasses}
        onClick={() => {
          action.run?.();
          onChosen();
        }}
      >
        {action.label}
      </a>
    );
  }
  return (
    <button
      type="button"
      className={actionClasses}
      onClick={() => {
        action.run?.();
        if (action.run !== undefined) {
          setCopied(true);
          if (timer.current !== null) clearTimeout(timer.current);
          timer.current = setTimeout(() => setCopied(false), TOAST_COPIED_RESET_MS);
        } else {
          onChosen();
        }
      }}
    >
      {copied ? "Copied!" : action.label}
    </button>
  );
}

function ToastCard({ toast, manager }: { toast: ToastItem; manager: ToastHostManager }): React.ReactElement {
  const { actionLabel, onAction, actions = [] } = actionOf(toast);
  return (
    <BaseToast.Root
      toast={toast}
      data-testid="toast-notification"
      onKeyDown={(event) => {
        if (event.key === "Escape") manager.close(toast.id);
      }}
      className="shadow-popover pointer-events-auto flex w-80 items-start gap-(--space-4) rounded-(--radius-popover) border border-(--border) bg-(--surface) p-(--space-8)"
    >
      <BaseToast.Content className="flex min-w-0 flex-1 flex-col gap-(--space-1)">
        <BaseToast.Title className="text-emphasis font-medium text-(--text)" />
        {toast.description ? <BaseToast.Description className="text-body text-(--text-muted)" /> : null}
        {actionLabel ? (
          <button
            type="button"
            onClick={() => {
              onAction?.();
              manager.close(toast.id);
            }}
            className="mt-(--space-2) cursor-pointer self-start font-medium text-(--accent) hover:text-(--accent-hover) focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-(--focus-ring-color) focus-visible:outline-solid"
          >
            {actionLabel}
          </button>
        ) : null}
        {actions.length > 0 ? (
          <div data-testid="toast-actions" className="mt-(--space-2) flex flex-wrap items-center gap-(--space-1)">
            {actions.map((action) => (
              <ToastActionItem key={action.label} action={action} onChosen={() => manager.close(toast.id)} />
            ))}
          </div>
        ) : null}
      </BaseToast.Content>
      <BaseToast.Close
        render={
          <IconButton label="Dismiss notification">
            <X size={16} strokeWidth={1.5} />
          </IconButton>
        }
      />
    </BaseToast.Root>
  );
}

function ToastList({ manager }: { manager: ToastHostManager }): React.ReactElement {
  const { toasts } = BaseToast.useToastManager();
  return (
    <>
      {toasts.map((toast) => (
        <ToastCard key={toast.id} toast={toast} manager={manager} />
      ))}
    </>
  );
}

export function ToastHost({ manager }: { manager: ToastHostManager }): React.ReactElement {
  return (
    <BaseToast.Provider toastManager={manager}>
      <BaseToast.Viewport
        data-testid="toast-stack"
        aria-label="Notifications"
        className="pointer-events-none fixed right-(--space-8) bottom-(--space-8) z-50 flex w-80 flex-col gap-(--space-4)"
      >
        <ToastList manager={manager} />
      </BaseToast.Viewport>
    </BaseToast.Provider>
  );
}
