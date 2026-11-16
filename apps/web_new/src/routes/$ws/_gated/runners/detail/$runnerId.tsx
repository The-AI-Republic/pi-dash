// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner detail, workspace scope (NEWFRONT-555, RUN-042/RUN-043). The area
// shell owns the role gate and rail; this route mounts the detail screen.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnerDetailContainer } from "../../../../../features/runners/detail/RunnerDetailContainer.js";

export const Route = createFileRoute("/$ws/_gated/runners/detail/$runnerId")({
  component: RunnerDetailRoute,
});

function RunnerDetailRoute(): React.ReactElement {
  const { ws, runnerId } = Route.useParams();
  return <RunnerDetailContainer workspaceSlug={ws} runnerId={runnerId} />;
}
