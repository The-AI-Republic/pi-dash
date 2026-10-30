// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy bare settings address (parity/specs/profile.md §5, PROF-026;
// main#670): forward to the general tab of the current workspace,
// query string kept.

import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/settings/")({
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
