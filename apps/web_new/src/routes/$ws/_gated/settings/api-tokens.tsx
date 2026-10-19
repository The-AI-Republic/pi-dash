// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy workspace api-tokens address (PROF-026): forward to the tokens
// tab with the query string kept (the NEWFRONT-431 target).

import { createFileRoute, redirect } from "@tanstack/react-router";

import { legacySettingsForward } from "../../../../features/settings/lib/legacyForwards.js";

export const Route = createFileRoute("/$ws/_gated/settings/api-tokens")({
  beforeLoad: ({ location }) => {
    const forward = legacySettingsForward(location.pathname, location.searchStr);
    if (forward !== null) throw redirect({ href: forward.href });
  },
});
