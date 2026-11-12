// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy workspace api-tokens address (PROF-026; main#670): forward to
// the account tokens tab with the query string kept (the NEWFRONT-431
// target).

import { createFileRoute, redirect } from "@tanstack/react-router";

import { accountTabForwardHref } from "../../../../features/settings/lib/accountHref.js";

export const Route = createFileRoute("/$ws/_gated/settings/api-tokens")({
  beforeLoad: ({ params, location }) => {
    throw redirect({ href: accountTabForwardHref(params.ws, "api-tokens", location) });
  },
});
