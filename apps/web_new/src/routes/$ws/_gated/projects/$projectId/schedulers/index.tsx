// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers section entry (parity/specs/agents.md §5, AGT-021):
// the bare tab address lands on the calendar, which NEWFRONT-593 builds.
// Until it lands the redirect target renders the section's not-found page,
// so the address and the redirect are stable for the calendar slice.

import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/schedulers/")({
  beforeLoad: ({ params }) => {
    throw redirect({
      href: `/${params.ws}/projects/${params.projectId}/schedulers/calendar`,
    });
  },
});
