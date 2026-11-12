// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare project address (SHELL-077). The address rewrites to the member's
// stored default tab, or to tracking when the stored key is unknown,
// gated away, or unreadable.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { DefaultTabRedirect } from "../../../../../features/projects/components/DefaultTabRedirect.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/")({
  component: BareProjectComponent,
});

function BareProjectComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return <DefaultTabRedirect workspaceSlug={ws} projectId={projectId} />;
}
