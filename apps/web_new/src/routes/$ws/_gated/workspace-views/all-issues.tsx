// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues route (parity/specs/issues.md §18, ISS-223). The
// address bar is the only input: the raw query parameters ride to the
// screen, which parses them into route filters. In this build the list
// ignores them (the oracle pins an unfiltered result for
// `?priority=urgent&order_by=-created_at`), but the deep link keeps its
// parameters.
//
// Route modules are part of the eager route graph, so this file holds
// nothing but the declaration: the router's component split carries the
// screen, its reads and the filter parser (Quality gates).

import { createFileRoute } from "@tanstack/react-router";

import { GlobalIssuesScreen } from "../../../../features/issues/global/GlobalIssuesScreen.js";

export const Route = createFileRoute("/$ws/_gated/workspace-views/all-issues")({
  component: GlobalIssuesScreen,
});
