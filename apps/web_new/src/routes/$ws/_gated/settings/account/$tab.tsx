// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy workspace account tab address (PROF-026): forward to the matching
// tab, defaulting an unknown id to general and keeping parameters.

import { createFileRoute, notFound, redirect } from "@tanstack/react-router";

import { legacySettingsForward } from "../../../../../features/settings/lib/legacyForwards.js";

export const Route = createFileRoute("/$ws/_gated/settings/account/$tab")({
  beforeLoad: ({ location }) => {
    const forward = legacySettingsForward(location.pathname, location.searchStr);
    if (forward === null) throw notFound();
    throw redirect({ href: forward.href });
  },
});
