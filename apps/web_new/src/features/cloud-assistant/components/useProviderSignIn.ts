// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider sign-in flow owned by the page (parity/specs/cloud.md §7,
// CLOUD-033). Polling lives here, not on the button: that button sits on
// the provider-account row only while the row is disconnected, and a
// refetch can report the row connected before the next poll does. Owned by
// the page, the poll still confirms and refreshes where the button would
// have unmounted.

import * as React from "react";

import { useCloudAssistantActions, type CloudAssistantActions } from "../api/mutations.js";
import { finishNotice, startFailureNotice } from "../lib/report.js";
import { beginProviderSignIn, pollProviderLogin, type PollClock, type PopupOpener } from "../lib/signIn.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

export interface ProviderSignIn {
  pending: boolean;
  signIn(): Promise<void>;
}

export interface ProviderSignInOptions {
  /** Refresh the list after the flow confirms. */
  onConfirmed?: (() => Promise<void> | void) | undefined;
  /** Test seams: the tab opener, poll clock and poll interval. */
  open?: PopupOpener | undefined;
  clock?: PollClock | undefined;
  intervalMs?: number | undefined;
}

export function useProviderSignIn(options: ProviderSignInOptions = {}): ProviderSignIn {
  const actions = useCloudAssistantActions();
  const [pending, setPending] = React.useState(false);
  const stopRef = React.useRef<{ stop(): void } | null>(null);

  React.useEffect(
    () => () => {
      stopRef.current?.stop();
    },
    []
  );

  const signIn = React.useCallback(async (): Promise<void> => {
    setPending(true);
    const open = options.open ?? (() => window.open("", "_blank"));
    const started = await beginProviderSignIn(open, () => actions.startSignIn());
    if (started.failure !== null) {
      setPending(false);
      const notice = startFailureNotice(started.failure);
      notifyError(notice.title, notice.body);
      return;
    }
    const state = started.result.state;
    stopRef.current?.stop();
    stopRef.current = pollProviderLogin({
      read: () => actions.signInStatus(state),
      ...(options.clock === undefined ? {} : { clock: options.clock }),
      ...(options.intervalMs === undefined ? {} : { intervalMs: options.intervalMs }),
      onFinish: (finish) => {
        const notice = finishNotice(finish);
        if (finish.kind === "confirmed") notifySuccess(notice.title, notice.body);
        else notifyError(notice.title, notice.body);
        setPending(false);
        void (finish.kind === "confirmed"
          ? Promise.resolve(options.onConfirmed?.()).then(() => actions.invalidate())
          : actions.invalidate());
      },
    });
  }, [actions, options]);

  return { pending, signIn };
}

/** The actions a page passes here; exported for the hook's test seam. */
export type { CloudAssistantActions };
