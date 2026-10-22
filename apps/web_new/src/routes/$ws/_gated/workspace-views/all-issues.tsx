// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues route (parity/specs/issues.md §18, ISS-223). The
// address bar is the only input; in this build the list ignores the raw
// query parameters (the oracle pins an unfiltered result for
// `?priority=urgent&order_by=-created_at`), but the deep link keeps its
// parameters. The empty surface is the workspace card (§8, ISS-073).
//
// Route modules are part of the eager route graph, so this file holds
// nothing but the declaration: the router's component split carries the
// screen and its reads (Quality gates).

import { createFileRoute } from "@tanstack/react-router";

import { GlobalIssuesScreen } from "../../../../features/issues/global/GlobalIssuesScreen.js";

export const Route = createFileRoute("/$ws/_gated/workspace-views/all-issues")({
  component: GlobalIssuesScreen,
});
