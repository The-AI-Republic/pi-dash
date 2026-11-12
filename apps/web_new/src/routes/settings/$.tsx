// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Every other old `/settings/...` address (parity/specs/profile.md §5,
// PROF-026; main#670). The old app's `settings/*` splat sent a stray
// leftover to the general tab rather than the not-found page, so a stale
// bookmark such as `/settings/account/security` keeps working; only the
// three legacy families the resolver names forward (this address lands on
// general), and anything outside them still answers the not-found page.

import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/settings/$")({
  beforeLoad: async ({ context, location }) => {
    const { loadLegacySettingsForward } = await import("../../features/settings/loadLegacyForward.js");
    const href = await loadLegacySettingsForward({
      queryClient: context.queryClient,
      apiClient: context.apiClient,
      pathname: location.pathname,
      search: location.searchStr,
      hash: location.hash,
    });
    throw redirect({ href });
  },
});
