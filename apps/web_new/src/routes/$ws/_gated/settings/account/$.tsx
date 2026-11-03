// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Deeper legacy workspace account addresses (PROF-026; main#670): an
// address below a tab, or one naming no registered tab, lands on this
// workspace's general tab with the query string kept rather than the
// not-found page a stale link would otherwise meet.

import { createFileRoute, redirect } from "@tanstack/react-router";

import { accountTabForwardHref } from "../../../../../features/settings/lib/accountHref.js";
import { DEFAULT_PROFILE_SETTINGS_TAB } from "../../../../../features/settings/lib/tabNavigation.js";

export const Route = createFileRoute("/$ws/_gated/settings/account/$")({
  beforeLoad: ({ params, location }) => {
    throw redirect({
      href: accountTabForwardHref(params.ws, DEFAULT_PROFILE_SETTINGS_TAB, location),
    });
  },
});
