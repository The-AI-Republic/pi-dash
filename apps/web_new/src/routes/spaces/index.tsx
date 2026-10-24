// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public landing (SPC-005, SPC-006). The search carries the sign-in
// return path, the email prefill and coded failures; the gate validates
// the return path and branches on the session. Public and outside the
// workspace shell, like every route in this subtree.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { LandingGate } from "../../features/spaces/components/LandingGate.js";
import { parseSpaceLandingSearch } from "../../features/spaces/lib/returnPath.js";

export const Route = createFileRoute("/spaces/")({
  validateSearch: parseSpaceLandingSearch,
  component: function SpacesLanding() {
    const search = Route.useSearch();
    return <LandingGate search={search} />;
  },
});
