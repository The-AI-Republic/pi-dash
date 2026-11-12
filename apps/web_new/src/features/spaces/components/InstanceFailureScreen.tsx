// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board instance failure (SPC-018). The instance probe is a single
// attempt, so a failed read lands here instead of retrying behind a spinner.
// Retry is a hard reload, which re-issues that one probe.

import { Button } from "@pidash/kit";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/document.js";

export const SPACE_INSTANCE_FAILURE_ART = "/illustrations/instance-failure.svg";
export const SPACE_INSTANCE_FAILURE_HEADING = "Unable to fetch instance details";

export function reloadSpacePage(): void {
  globalThis.window.location.reload();
}

export function SpaceInstanceFailureScreen(): React.ReactElement {
  useDocumentHead({ title: "Pi Dash — Instance unavailable", robots: "noindex, nofollow" });
  return (
    <main
      data-testid="space-instance-failure"
      className="flex min-h-screen flex-col items-center justify-center gap-(--space-5) bg-(--bg) px-(--space-8) text-center text-(--text)"
    >
      <a href="/" className="text-emphasis flex items-center gap-(--space-2)">
        <img src="/icon.svg" alt="" width={28} height={28} />
        <span>Pi Dash</span>
      </a>
      <img src={SPACE_INSTANCE_FAILURE_ART} alt="An unavailable server" width={320} height={240} />
      <h1 className="text-h2">{SPACE_INSTANCE_FAILURE_HEADING}</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        The instance could not be reached. Check your connection, then try again.
      </p>
      <Button variant="primary" onClick={reloadSpacePage}>
        Retry
      </Button>
    </main>
  );
}
