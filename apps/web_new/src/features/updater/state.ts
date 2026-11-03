// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-update state for the sidebar control (DESK-018). The shell owns
// which update waits and whether it is downloading; this hook only keeps a
// page-side mirror so the control can re-render. Every mount asks the shell
// again (an update deferred at launch may predate the control), and every
// running session follows later announcements.

import * as React from "react";

import type { PendingUpdate, UpdaterApi } from "../../core/platform/index.js";

/** Shown when the shell reports no specific reason for a failed install. */
export const UPDATE_FAILED_FALLBACK_TEXT = "The update could not be installed.";

/** Shown if the install call returns without restarting the app. */
export const UPDATE_NO_RESTART_TEXT = "The app did not restart into the new version.";

export interface UpdaterState {
  /** The release the shell is holding, or null while none waits. */
  update: PendingUpdate | null;
  /** Busy because an install this page started is still running. */
  requesting: boolean;
  /** Why the last install attempt failed, or null. */
  failure: string | null;
  /** Start (or retry) the install. Does nothing without an update. */
  install: () => void;
}

/** Read a rejected install's reason without leaking "Error:" prefixes. */
export function installFailureText(error: unknown): string {
  const text = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  const trimmed = text.trim();
  return trimmed.length > 0 ? trimmed : UPDATE_FAILED_FALLBACK_TEXT;
}

/**
 * Mirror the shell's pending update. `updates` is absent on the web bundle,
 * where nothing waits and nothing renders.
 */
export function useUpdaterState(updates: UpdaterApi | undefined): UpdaterState {
  const [update, setUpdate] = React.useState<PendingUpdate | null>(null);
  const [requesting, setRequesting] = React.useState(false);
  const [failure, setFailure] = React.useState<string | null>(null);
  const updateRef = React.useRef<PendingUpdate | null>(null);
  const attemptRef = React.useRef(0);

  React.useEffect(() => {
    // A capability change starts a fresh mirror: the old release, its
    // failure and any in-flight request belong to the previous platform.
    attemptRef.current += 1;
    updateRef.current = null;
    setUpdate(null);
    setRequesting(false);
    setFailure(null);
    if (!updates) return;
    let mounted = true;
    // An announcement can race the pending query. Whichever arrives first
    // wins: a later query answer would be the older fact.
    let announced = false;
    const apply = (next: PendingUpdate | null): void => {
      const previousVersion = updateRef.current?.version ?? null;
      updateRef.current = next;
      setUpdate(next);
      if (next !== null && previousVersion !== next.version) setFailure(null);
    };
    const unsubscribe = updates.onAvailable((next) => {
      if (!mounted) return;
      announced = true;
      apply(next);
    });
    void updates.pending().then(
      (pending) => {
        if (mounted && !announced) apply(pending);
      },
      // A refused query leaves the control hidden; the next mount asks
      // again and the event keeps working.
      () => undefined
    );
    return () => {
      mounted = false;
      unsubscribe();
    };
  }, [updates]);

  const install = React.useCallback(() => {
    if (!updates || update === null) return;
    const attempt = ++attemptRef.current;
    setRequesting(true);
    setFailure(null);
    void updates.install().then(
      // A successful install restarts the app, so a plain resolve means the
      // install did not finish.
      () => {
        if (attempt !== attemptRef.current) return;
        setRequesting(false);
        setFailure(UPDATE_NO_RESTART_TEXT);
      },
      (error: unknown) => {
        if (attempt !== attemptRef.current) return;
        setRequesting(false);
        setFailure(installFailureText(error));
      }
    );
  }, [updates, update]);

  return { update, requesting, failure, install };
}
