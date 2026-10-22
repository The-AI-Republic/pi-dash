// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Retired project-inbox address (parity/specs/intake.md INT-001). Older
// links and bookmarks used `/projects/<id>/inbox`; each one forwards to
// the intake screen of the same workspace and project, keeping nothing
// else. The redirect fires before any page paints.

import { createFileRoute, redirect } from "@tanstack/react-router";

import { intakeHref } from "../../../../../features/intake/lib/paths.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/inbox")({
  beforeLoad: ({ params }) => {
    throw redirect({ href: intakeHref(params.ws, params.projectId) });
  },
});
