// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner detail mount point (NEWFRONT-540, RUN-010). The Details row action
// deep-links here in both scopes; the detail screen itself lands with
// NEWFRONT-555.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnersAreaSectionMount } from "../../../../../features/runners/components/RunnersAreaSectionMount.js";

export const Route = createFileRoute("/$ws/_gated/runners/detail/$runnerId")({
  component: RunnerDetailMountRoute,
});

function RunnerDetailMountRoute(): React.ReactElement {
  return <RunnersAreaSectionMount title="Runner details" />;
}
