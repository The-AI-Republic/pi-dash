// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace catch-all (SHELL-108). Unknown addresses render the
// illustrated not-found page with a way home, with or without a session:
// this route sits outside the gated layout, so the sign-in gate never
// preempts it. The membership gate in the workspace route still rejects
// unknown slugs for signed-in visitors before this route is reached.

import { createFileRoute } from "@tanstack/react-router";

import { NotFoundPage } from "../../shared/shell/index.js";

export const Route = createFileRoute("/$ws/$")({
  component: NotFoundPage,
});
