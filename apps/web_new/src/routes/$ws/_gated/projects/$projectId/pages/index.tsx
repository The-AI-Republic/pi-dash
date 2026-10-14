// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages tab home (SHELL-075). Thin mount: the Pages area epic owns this
// screen and replaces the mount with its content when it lands.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { TabAreaMount } from "../../../../../../features/projects/index.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/pages/")({
  component: PagesComponent,
});

function PagesComponent(): React.ReactElement {
  return <TabAreaMount area="Pages" />;
}
