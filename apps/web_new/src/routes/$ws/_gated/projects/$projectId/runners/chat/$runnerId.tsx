// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner chat mount point, project scope (NEWFRONT-540). Row actions and rail contacts
// link here; the screen itself lands with its owning slice.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnersAreaSectionMount } from "../../../../../../../features/runners/components/RunnersAreaSectionMount.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/runners/chat/$runnerId")({
  component: ProjectRunnerMountRoute,
});

function ProjectRunnerMountRoute(): React.ReactElement {
  return <RunnersAreaSectionMount title="Runner chat" />;
}
