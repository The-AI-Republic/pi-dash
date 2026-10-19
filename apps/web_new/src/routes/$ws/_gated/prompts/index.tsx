// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompts home (parity/specs/agents.md §6-7, AGT-023, AGT-028). Thin:
// resolves the workspace slug and mounts the prompts page (sections
// browser and receipt tab). The key remounts per workspace so tab, nav
// and editor state never leak across workspaces.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { PromptsView } from "../../../../features/prompts/index.js";

export const Route = createFileRoute("/$ws/_gated/prompts/")({
  component: PromptsComponent,
});

function PromptsComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  return <PromptsView key={ws} workspaceSlug={ws} />;
}
