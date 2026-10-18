// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs tab mount point (NEWFRONT-540, RUN-003). The runs list and detail land
// with NEWFRONT-545; this route keeps the tab URL-driven and deep-linkable in
// both scopes until then, mirroring the shell's area mounts.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnersAreaSectionMount } from "../../../../features/runners/components/RunnersAreaSectionMount.js";

export const Route = createFileRoute("/$ws/_gated/runners/runs")({
  component: RunsMountRoute,
});

function RunsMountRoute(): React.ReactElement {
  return <RunnersAreaSectionMount title="Runs" />;
}
