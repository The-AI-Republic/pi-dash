// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Retired per-project settings prefix (PSET-003; from
// parity/specs/project-settings-shell.md). Older links and bookmarks put
// settings under the project itself; this route sends each one to the
// workspace settings address, keeping every segment after the project id
// so the same tab opens. Nothing renders here — the redirect fires before
// the page paints.

import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/settings/$")({
  beforeLoad: ({ location, params }) => {
    const retiredPrefix = `/${params.ws}/projects/${params.projectId}/settings`;
    const suffix = location.pathname.startsWith(retiredPrefix) ? location.pathname.slice(retiredPrefix.length) : "";
    const trailing = suffix === "" || suffix.endsWith("/") ? "" : "/";
    throw redirect({ href: `/${params.ws}/settings/projects/${params.projectId}${suffix}${trailing}` });
  },
});
