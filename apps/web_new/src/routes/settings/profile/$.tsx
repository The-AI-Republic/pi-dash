// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy `/settings/profile` and `/settings/profile/<tab>` addresses
// (parity/specs/profile.md §5, PROF-026; main#670). Both forms land on
// the same tab inside the current workspace; a deeper address is not a
// forward and gets the not-found page.

import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/settings/profile/$")({
  beforeLoad: async ({ context, location }) => {
    const { loadLegacySettingsForward } = await import("../../../features/settings/loadLegacyForward.js");
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
