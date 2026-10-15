// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud retryable error screen (ADM-078, from parity/specs/admin.md
// §10.5): a transient admin-check or probe failure lands here — never a
// deny and never an OIDC bounce, so a backend blip strands nobody.
// Retry re-runs the admin check. The heading and the retry label are
// pinned by the parity scenario; the surrounding copy is fresh. Cloud
// only: the OSS build never renders this screen.
import { Button } from "@pidash/kit";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";

export const CLOUD_ENTRY_ERROR_TITLE = "Something went wrong — Pi Dash instance admin";

export function CloudEntryErrorScreen({ onRetry }: { onRetry: () => void }): React.ReactElement {
  useDocumentHead({ title: CLOUD_ENTRY_ERROR_TITLE, robots: "noindex, nofollow" });
  return (
    <main
      data-testid="admin-cloud-error"
      className="flex min-h-screen flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center"
    >
      <h1 className="text-h2 text-(--text)">Something went wrong</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        The admin console could not load. Check your connection, then try again.
      </p>
      <Button variant="primary" onClick={onRetry}>
        Try again
      </Button>
    </main>
  );
}
