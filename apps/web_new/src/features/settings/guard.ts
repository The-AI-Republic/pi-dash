// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Route guard shared by the profile-settings shell and its legacy
// forwards (parity/specs/profile.md §5). Settings are signed-in-only:
// visitors bounce to the entry with the return path and unfinished
// members continue onboarding, exactly like the notifications tab.

import { redirect } from "@tanstack/react-router";
import type { QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";

import { decidePageGate, isOnboardedProfile, loadGateInputs, ONBOARDING_PATH } from "../../core/session/index.js";

export async function enforceProfileSettingsGate(args: {
  queryClient: QueryClient;
  apiClient: ApiClient;
  href: string;
}): Promise<void> {
  const { me, profile } = await loadGateInputs(args.queryClient, args.apiClient);
  const decision = decidePageGate({
    page: "signed-in-only",
    signedIn: me !== null,
    onboarded: isOnboardedProfile(profile),
    hasPassword: me?.is_password_autoset === false,
    currentPath: args.href,
  });
  if (decision.action === "sign-in") {
    throw redirect({ to: "/", search: { next_path: decision.nextPath } });
  }
  if (decision.action === "onboarding") {
    throw redirect({ href: ONBOARDING_PATH });
  }
}
