// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { Toast as BaseToast } from "@base-ui-components/react/toast";
import { X } from "lucide-react";
import * as React from "react";
import { IconButton } from "./IconButton";

export type ToastHostManager = ReturnType<typeof BaseToast.createToastManager>;

type ToastItem = ReturnType<typeof BaseToast.useToastManager>["toasts"][number];

interface ToastActionData {
  actionLabel?: string | undefined;
  onAction?: (() => void) | undefined;
}

export function createKitToastManager(): ToastHostManager {
  return BaseToast.createToastManager();
}

export interface ShowToastOptions {
  title: string;
  description?: string;
  actionLabel?: string;
  onAction?: () => void;
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
    },
  });
}

function actionOf(toast: ToastItem): ToastActionData {
  return (toast.data as ToastActionData | undefined) ?? {};
}

function ToastCard({ toast, manager }: { toast: ToastItem; manager: ToastHostManager }): React.ReactElement {
  const { actionLabel, onAction } = actionOf(toast);
  return (
    <BaseToast.Root
      toast={toast}
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
      <BaseToast.Viewport className="pointer-events-none fixed right-(--space-8) bottom-(--space-8) z-50 flex w-80 flex-col gap-(--space-4)">
        <ToastList manager={manager} />
      </BaseToast.Viewport>
    </BaseToast.Provider>
  );
}
