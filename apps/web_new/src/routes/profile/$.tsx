// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy /profile and /profile/<tab> addresses (PROF-026): forward to the
// matching settings tab, defaulting an absent or unknown id to general.
// One splat covers both shapes; a deeper address is not a profile forward
// and gets the not-found page.

import { createFileRoute, notFound, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/profile/$")({
  beforeLoad: async ({ location }) => {
    const { legacySettingsForward } = await import("../../features/settings/lib/legacyForwards.js");
    const forward = legacySettingsForward(location.pathname, location.searchStr);
    if (forward === null) throw notFound();
    throw redirect({ href: forward.href });
  },
});
