// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud terminal not-authorized screen (ADM-077, from
// parity/specs/admin.md §10.4): a signed-in visitor who is not on the
// instance-admin allowlist lands here with no redirect, no OIDC bounce,
// and no switch-account action — only a plain full-navigation link back
// to the site root, so the screen can never loop. The heading and the
// link label are pinned by the parity scenario; the surrounding copy is
// fresh. Cloud only: the OSS build never renders this screen.
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";

export const CLOUD_ACCESS_DENIED_TITLE = "Access denied — Pi Dash instance admin";

export function CloudAccessDeniedScreen(): React.ReactElement {
  useDocumentHead({ title: CLOUD_ACCESS_DENIED_TITLE, robots: "noindex, nofollow" });
  return (
    <main
      data-testid="admin-cloud-denied"
      className="flex min-h-screen flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center"
    >
      <h1 className="text-h2 text-(--text)">Access denied</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        This account is signed in, but it is not an instance admin, so it cannot open the admin console.
      </p>
      <a href="/" className="text-body font-medium text-(--accent) hover:underline">
        Return to Pi Dash
      </a>
    </main>
  );
}
