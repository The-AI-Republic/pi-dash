// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Session-expired redirect (AUTH-020). Any 401 already moves the session
// store to expired (see core/session/middleware); this effect notices once
// and sends the user to the sign-in entry (/) with the return path
// preserved as next_path (/sign-in is a retired bare redirect that drops
// params, so it cannot carry the return path). The app shell and every
// authenticated surface outside it (the set-password page) share it.

import { useRouter } from "@tanstack/react-router";
import * as React from "react";

import { useSessionStore } from "../../core/session/store.js";
import { notifyError } from "./toaster.js";

export function useSessionExpiredRedirect(): void {
  const router = useRouter();
  const status = useSessionStore((state) => state.status);
  const returnUrl = useSessionStore((state) => state.returnUrl);
  const expiredNoticed = React.useRef(false);

  React.useEffect(() => {
    if (status !== "expired" || expiredNoticed.current) return;
    expiredNoticed.current = true;
    notifyError("Session expired", "Sign in again to continue.");
    const current = router.state.location.href;
    void router.navigate({ to: "/", search: { next_path: returnUrl ?? current } });
  }, [status, returnUrl, router]);
}
