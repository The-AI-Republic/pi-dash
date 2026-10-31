// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance fetch-failure screen (ADM-003, from parity/specs/admin.md
// §1.3): when the single instance-info read errors, an explanatory
// screen whose retry is a full page reload. No silent auto-retry, no
// retry counter.

import { Button } from "@pidash/kit";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";

export const INSTANCE_FAILURE_ART = "/illustrations/instance-failure.svg";
export const INSTANCE_FAILURE_TITLE = "Instance unavailable — Pi Dash instance admin";

/** Manual retry: a full reload re-issues the single info fetch. */
export function reloadEntryPage(): void {
  globalThis.window.location.reload();
}

export function InstanceFailureScreen(): React.ReactElement {
  useDocumentHead({ title: INSTANCE_FAILURE_TITLE, robots: "noindex, nofollow" });
  return (
    <main
      data-testid="admin-failure"
      className="flex min-h-screen flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center"
    >
      <img
        src={INSTANCE_FAILURE_ART}
        alt="A server cabinet with its link light out"
        width={320}
        height={240}
        className="text-(--text-muted)"
      />
      <h1 className="text-h2 text-(--text)">Could not reach this instance</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        The instance details failed to load. Check your connection, then retry.
      </p>
      <Button variant="primary" onClick={reloadEntryPage}>
        Retry
      </Button>
    </main>
  );
}
