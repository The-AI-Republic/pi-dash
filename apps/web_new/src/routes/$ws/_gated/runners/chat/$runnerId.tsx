// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner chat mount point (NEWFRONT-540, RUN-004). Every rail contact links
// here, so the address resolves in both scopes; the chat page lands with
// NEWFRONT-549.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnersAreaSectionMount } from "../../../../../features/runners/components/RunnersAreaSectionMount.js";

export const Route = createFileRoute("/$ws/_gated/runners/chat/$runnerId")({
  component: RunnerChatMountRoute,
});

function RunnerChatMountRoute(): React.ReactElement {
  return <RunnersAreaSectionMount title="Runner chat" />;
}
