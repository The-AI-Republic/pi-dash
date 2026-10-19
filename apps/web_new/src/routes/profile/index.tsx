// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy bare profile-settings address (PROF-026): forward to general.

import { createFileRoute, redirect } from "@tanstack/react-router";

import { legacySettingsForward } from "../../features/settings/lib/legacyForwards.js";

export const Route = createFileRoute("/profile/")({
  beforeLoad: ({ location }) => {
    const forward = legacySettingsForward(location.pathname, location.searchStr);
    if (forward !== null) throw redirect({ href: forward.href });
  },
});
