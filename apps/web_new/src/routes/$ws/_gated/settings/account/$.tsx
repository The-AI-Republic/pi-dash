// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy workspace account settings addresses (PROF-026): the bare
// address and the per-tab one both forward to the profile settings tab
// they name, keeping the query string. One splat covers both shapes.

import { createFileRoute, notFound, redirect } from "@tanstack/react-router";

import { legacySettingsForward } from "../../../../../features/settings/lib/legacyForwards.js";

export const Route = createFileRoute("/$ws/_gated/settings/account/$")({
  beforeLoad: ({ location }) => {
    const forward = legacySettingsForward(location.pathname, location.searchStr);
    if (forward === null) throw notFound();
    throw redirect({ href: forward.href });
  },
});
