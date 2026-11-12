// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy workspace/project address (SPC-002). Resolution happens in the
// component against the public endpoint; the route itself is not gated.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { LegacyResolver } from "../../../features/spaces/components/LegacyResolver.js";
import { parseSpaceBoardSearch } from "../../../features/spaces/lib/address.js";

export const Route = createFileRoute("/spaces/$workspaceSlug/$projectId")({
  validateSearch: parseSpaceBoardSearch,
  component: function LegacySpaceRoute() {
    const { workspaceSlug, projectId } = Route.useParams();
    const search = Route.useSearch();
    return <LegacyResolver workspaceSlug={workspaceSlug} projectId={projectId} search={search} />;
  },
});
