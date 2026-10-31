// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare analytics address (parity/specs/analytics.md §1, ANL-001).
// Forwards to the default tab, keeping the query string so a scoped
// bookmark still lands scoped. Nothing renders here — the redirect
// fires before the page paints.

import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/$ws/_gated/analytics/")({
  beforeLoad: async ({ location, params }) => {
    const { defaultAnalyticsTabHref } = await import("../../../../features/analytics/lib/tabs.js");
    throw redirect({ href: defaultAnalyticsTabHref(params.ws, location.searchStr) });
  },
});
