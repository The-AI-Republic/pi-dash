// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Schedulers tab home (SHELL-075). Thin mount: the Schedulers area epic owns this
// screen and replaces the mount with its content when it lands.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { TabAreaMount } from "../../../../../../features/projects/index.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/schedulers/")({
  component: SchedulersComponent,
});

function SchedulersComponent(): React.ReactElement {
  return <TabAreaMount area="Schedulers" />;
}
