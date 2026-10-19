// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant landing route (parity/specs/agents.md §9, AGT-038–AGT-040).
// Thin: the feature owns the greeting/setup swap and the start action.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AssistantLanding } from "../../../../features/assistant/index.js";

export const Route = createFileRoute("/$ws/_gated/assistant/")({
  component: AssistantLandingComponent,
});

function AssistantLandingComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  return <AssistantLanding key={ws} workspaceSlug={ws} />;
}
