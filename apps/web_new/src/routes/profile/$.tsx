// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy /profile and /profile/<tab> addresses (PROF-026; main#670):
// forward to the same tab inside the current workspace, defaulting an
// absent or unknown id to general. A deeper address is not a profile
// forward and gets the not-found page.

import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/profile/$")({
  beforeLoad: async ({ context, location }) => {
    const { loadLegacySettingsForward } = await import("../../features/settings/loadLegacyForward.js");
    const href = await loadLegacySettingsForward({
      queryClient: context.queryClient,
      apiClient: context.apiClient,
      pathname: location.pathname,
      search: location.searchStr,
    });
    throw redirect({ href });
  },
});
