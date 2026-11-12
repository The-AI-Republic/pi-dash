// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare account-settings address (parity/specs/profile.md §5, PROF-009;
// main#670): forward to the general tab, keeping the query string.

import { createFileRoute, redirect } from "@tanstack/react-router";

import { accountTabForwardHref } from "../../../../../features/settings/lib/accountHref.js";
import { DEFAULT_PROFILE_SETTINGS_TAB } from "../../../../../features/settings/lib/tabNavigation.js";

export const Route = createFileRoute("/$ws/_gated/settings/account/")({
  beforeLoad: ({ params, location }) => {
    throw redirect({
      href: accountTabForwardHref(params.ws, DEFAULT_PROFILE_SETTINGS_TAB, location),
    });
  },
});
