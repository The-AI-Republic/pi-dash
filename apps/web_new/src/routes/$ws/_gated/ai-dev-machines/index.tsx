// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI Dev Machines home (NEWFRONT-555, RUN-037–041). The layout above owns the
// role gate; this route mounts the machines screen.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { DevMachinesContainer } from "../../../../features/runners/devMachines/DevMachinesContainer.js";

export const Route = createFileRoute("/$ws/_gated/ai-dev-machines/")({
  component: AiDevMachinesComponent,
});

function AiDevMachinesComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  return <DevMachinesContainer workspaceSlug={ws} />;
}
