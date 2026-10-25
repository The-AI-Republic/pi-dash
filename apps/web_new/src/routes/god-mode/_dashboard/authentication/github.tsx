// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// GitHub provider sub-page route (ADM-027–028, from
// parity/specs/admin.md §4.5–4.6): the shared provider shell over the
// GitHub definition with the GitHub credential field set.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminAuthProviderPage } from "../../../../features/admin/components/AdminAuthProviderPage.js";
import { AUTH_PROVIDER_BY_SLUG } from "../../../../features/admin/providerAuth.js";
import { PROVIDER_CREDENTIALS } from "../../../../features/admin/providerCredentials.js";

export const Route = createFileRoute("/god-mode/_dashboard/authentication/github")({
  component: GithubAuthenticationRoute,
});

function GithubAuthenticationRoute(): React.ReactElement {
  const credentials = PROVIDER_CREDENTIALS.github;
  return (
    <AdminAuthProviderPage
      provider={AUTH_PROVIDER_BY_SLUG.github}
      fields={credentials.fields}
      prefill={credentials.prefill}
      copyDescriptions={credentials.copyDescriptions}
      copyHelp={credentials.copyHelp}
    />
  );
}
