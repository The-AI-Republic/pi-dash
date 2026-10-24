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
  actions?: readonly ToastAction[] | undefined;
}

/** One inline action row on a toast. */
export interface ToastAction {
  /** Visible label, e.g. "View work item" or "Copy link". */
  label: string;
  /**
   * Runs when the action is chosen. A copy action may answer false when
   * the write failed, so the confirmation is only claimed on success.
   */
  run?: (() => void | boolean | Promise<void | boolean>) | undefined;
  /** Render as an anchor opened in a new tab at this address. */
  href?: string | undefined;
  /**
   * Swap the label for this confirmation for a few seconds once the
   * action reports success, instead of dismissing the notice. A copy
   * action uses it so the user sees the write land.
   */
  confirmLabel?: string | undefined;
}

export function createKitToastManager(): ToastHostManager {
  return BaseToast.createToastManager();
}

export interface ShowToastOptions {
  title: string;
  description?: string;
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
    ...(options.actions === undefined ? {} : { data: { actions: options.actions } }),
  });
}

function actionOf(toast: ToastItem): ToastActionData {
  return (toast.data as ToastActionData | undefined) ?? {};
}

/** Action rows read as links on the notice; the copy row confirms in place. */
const ACTION_CLASSES =
  "inline-flex cursor-pointer items-center rounded-(--radius-control) px-(--space-2) py-px font-medium text-(--accent) hover:text-(--accent-hover)";

function ToastCard({ toast, manager }: { toast: ToastItem; manager: ToastHostManager }): React.ReactElement {
  const { actions = [] } = actionOf(toast);
  const [copied, setCopied] = React.useState<string | null>(null);
  const timer = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  React.useEffect(
    () => () => {
      if (timer.current !== null) clearTimeout(timer.current);
    },
    []
  );
  // A copy action reports whether the write landed; only then does its row
  // swap to the confirmation, and it swaps back after a moment.
  const choose = (action: ToastAction) => {
    const close = () => manager.close(toast.id);
    if (action.run === undefined) {
      close();
      return;
    }
    if (action.confirmLabel === undefined) {
      action.run();
      close();
      return;
    }
    void Promise.resolve(action.run()).then((ok) => {
      if (ok === false) return;
      setCopied(action.label);
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(null), TOAST_COPIED_RESET_MS);
    });
  };
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
        {actions.length > 0 ? (
          <div data-testid="toast-actions" className="mt-(--space-2) flex flex-wrap items-center gap-(--space-1)">
            {actions.map((action) =>
              action.href === undefined ? (
                <button key={action.label} type="button" className={ACTION_CLASSES} onClick={() => choose(action)}>
                  {copied === action.label ? action.confirmLabel : action.label}
                </button>
              ) : (
                <a
                  key={action.label}
                  href={action.href}
                  target="_blank"
                  rel="noopener noreferrer"
                  className={ACTION_CLASSES}
                  onClick={() => {
                    action.run?.();
                    manager.close(toast.id);
                  }}
                >
                  {action.label}
                </a>
              )
            )}
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
  const toastsRef = React.useRef(toasts);
  toastsRef.current = toasts;
  // The provider keeps an auto-dismiss timer per timed notice but never
  // clears them when it unmounts, so a timer can fire after its host is
  // gone (in tests, after the jsdom window is torn down, which throws).
  // Closing each notice here clears its timer while the provider can still
  // accept the state update. It is a layout effect so the cleanup runs
  // before the provider's passive unsubscribe, keeping the close delivery.
  React.useLayoutEffect(
    () => () => {
      for (const toast of toastsRef.current) manager.close(toast.id);
    },
    [manager]
  );
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
