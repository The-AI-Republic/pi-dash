// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompts home (SHELL-054). Thin mount: the prompts area epic owns
// this screen and replaces the mount with its content when it lands.
// Until then the address renders inside the workspace layout so the
// More disclosure auto-opens with its row active instead of a dead end.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { TabAreaMount } from "../../../../features/projects/index.js";

export const Route = createFileRoute("/$ws/_gated/prompts/")({
  component: PromptsComponent,
});

function PromptsComponent(): React.ReactElement {
  return <TabAreaMount area="Prompts" />;
}
