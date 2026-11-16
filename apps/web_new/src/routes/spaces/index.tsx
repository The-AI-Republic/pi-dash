// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public landing (SPC-005, SPC-006). The search carries the sign-in
// return path, the email prefill and coded failures; the gate validates
// the return path and branches on the session. Public and outside the
// workspace shell, like every route in this subtree. Slice D fills the
// gate's badge and account slots: the product badge on the auth and
// guidance views (SPC-012), and the shared account menu on the guidance
// view (SPC-014) so a signed-in visitor can sign out from here.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { LandingGate } from "../../features/spaces/components/LandingGate.js";
import { SpaceLandingAccount } from "../../features/spaces/components/SpaceLandingAccount.js";
import { SpaceProductBadge } from "../../features/spaces/components/SpaceProductBadge.js";
import { parseSpaceLandingSearch } from "../../features/spaces/lib/returnPath.js";

export const Route = createFileRoute("/spaces/")({
  validateSearch: parseSpaceLandingSearch,
  component: function SpacesLanding() {
    const search = Route.useSearch();
    return (
      <LandingGate search={search} badge={<SpaceProductBadge />} account={<SpaceLandingAccount search={search} />} />
    );
  },
});
