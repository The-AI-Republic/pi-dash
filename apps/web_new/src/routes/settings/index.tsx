// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy bare settings address (parity/specs/profile.md §5, PROF-026):
// forward to the general tab with the query string kept.

import { createFileRoute, redirect } from "@tanstack/react-router";

import { legacySettingsForward } from "../../features/settings/lib/legacyForwards.js";

export const Route = createFileRoute("/settings/")({
  beforeLoad: ({ location }) => {
    const forward = legacySettingsForward(location.pathname, location.searchStr);
    if (forward !== null) throw redirect({ href: forward.href });
  },
});
