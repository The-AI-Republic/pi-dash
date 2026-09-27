/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { CircleArrowUp } from "lucide-react";
import { IconButton } from "@pi-dash/propel/icon-button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Tooltip } from "@pi-dash/propel/tooltip";
import { isDesktop } from "@/services/agent-runtime";

/** Emitted by the desktop (`updates.rs`) when the daily check finds an update. */
export const UPDATE_AVAILABLE_EVENT = "updater://available";

type PendingUpdate = { version: string; currentVersion: string; installing: boolean };

interface TauriGlobal {
  core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> };
  event: { listen<T>(event: string, handler: (e: { payload: T }) => void): Promise<() => void> };
}

function tauri(): TauriGlobal {
  return (window as unknown as { __TAURI__: TauriGlobal }).__TAURI__;
}

/**
 * Corner button beside the sidebar's user menu, shown while an update is
 * waiting: found by the daily check, or deferred with "Later" at launch.
 * Clicking it downloads and installs the update; the app then restarts.
 */
export function DesktopUpdateButton() {
  const [update, setUpdate] = useState<PendingUpdate | null>(null);
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    if (!isDesktop()) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    // The desktop owns both facts: which update is waiting, and whether it
    // is downloading right now. A remount that landed mid-download must not
    // show an enabled button — clicking it would only raise "an update is
    // already installing".
    const apply = (pending: PendingUpdate) => {
      setUpdate(pending);
      setInstalling(Boolean(pending.installing));
    };
    const subscribe = async () => {
      const stop = await tauri().event.listen<PendingUpdate>(UPDATE_AVAILABLE_EVENT, (e) => apply(e.payload));
      if (active) unlisten = stop;
      else stop();
    };
    // The sidebar remounts on collapse and route changes; the event may
    // already have fired, so ask for the update that is waiting.
    const loadPending = async () => {
      const pending = await tauri().core.invoke<PendingUpdate | null>("desktop_pending_update");
      if (active && pending) apply(pending);
    };
    subscribe().catch(() => undefined);
    loadPending().catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  if (!update) return null;

  const install = async () => {
    setInstalling(true);
    try {
      // Resolves only on failure: success restarts the app.
      await tauri().core.invoke("desktop_install_update");
    } catch (error) {
      setInstalling(false);
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Update failed",
        message: String(error instanceof Error ? error.message : error),
      });
    }
  };

  return (
    <Tooltip
      tooltipContent={
        installing ? "Installing update…" : `Update to Pi Dash ${update.version} (you have ${update.currentVersion})`
      }
      position="top"
    >
      <IconButton
        size="lg"
        variant="primary"
        icon={CircleArrowUp}
        loading={installing}
        onClick={() => void install()}
        aria-label={`Update to Pi Dash ${update.version}`}
        className="shrink-0"
      />
    </Tooltip>
  );
}
