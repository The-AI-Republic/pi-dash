// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Google provider sub-page route (ADM-027, from parity/specs/admin.md
// §4.5): the shared provider shell over the Google definition.
// Credential field sets arrive in NEWFRONT-511.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminAuthProviderPage, AUTH_PROVIDER_BY_SLUG } from "../../../../features/admin/index.js";

export const Route = createFileRoute("/god-mode/_dashboard/authentication/google")({
  component: GoogleAuthenticationRoute,
});

function GoogleAuthenticationRoute(): React.ReactElement {
  return <AdminAuthProviderPage provider={AUTH_PROVIDER_BY_SLUG.google} />;
}
