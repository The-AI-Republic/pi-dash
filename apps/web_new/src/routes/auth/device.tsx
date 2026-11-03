// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// CLI device-approval page (AUTH-021). Signed-in users land here from the
// terminal flow, optionally carrying ?code=; the card prefills and tries
// once, then stays editable for retries.

import { ToastHost } from "@pidash/kit";
import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { decidePageGate, loadGateInputs, ONBOARDING_PATH } from "../../core/session/index.js";
import { asString } from "../../features/auth/entrySearch.js";
import { DeviceApprovalCard } from "../../features/auth/components/DeviceApprovalCard.js";
import { getToastManager, useSessionExpiredRedirect } from "../../shared/shell/index.js";

/** Lenient CLI-carried code; anything else falls back to absent. */
export function parseDeviceSearch(search: Record<string, unknown>): { code?: string | undefined } {
  const code = asString(search["code"]);
  return code === undefined ? {} : { code };
}

export const Route = createFileRoute("/auth/device")({
  validateSearch: parseDeviceSearch,
  beforeLoad: async ({ context, location }) => {
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "signed-in-only",
      signedIn: me !== null,
      onboarded: profile?.is_onboarded ?? false,
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "sign-in") {
      throw redirect({ to: "/", search: { next_path: decision.nextPath } });
    }
    if (decision.action === "onboarding") {
      throw redirect({ href: ONBOARDING_PATH });
    }
    return {};
  },
  // Inline wrapper (not a reference to the exported screen): only this
  // form lets the router split the route component out of the entry.
  component: function DeviceComponent() {
    return <DeviceScreen />;
  },
});

export function DeviceScreen(): React.ReactElement {
  const { code } = Route.useSearch();
  useSessionExpiredRedirect();
  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg)">
      <DeviceApprovalCard initialCode={code} />
      <ToastHost manager={getToastManager()} />
    </main>
  );
}
