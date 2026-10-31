// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare archives address (parity/specs/archives.md §1.1, ARCH-001). No
// index screen exists here; shared links to the bare address land on
// the work-items tab instead of a 404 or an empty frame. The redirect
// carries no feature import so this component-less route stays out of
// the initial-JS budget beyond its own tree entry.

import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/")({
  beforeLoad: ({ params }) => {
    throw redirect({
      to: "/$ws/projects/$projectId/archives/issues",
      params: { ws: params.ws, projectId: params.projectId },
    });
  },
});
