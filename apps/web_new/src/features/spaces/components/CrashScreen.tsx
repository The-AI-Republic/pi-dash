// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board render crash (SPC-019). The boundary is scoped to the space
// subtree, so a broken payload replaces only this screen and a reload with
// a healthy payload returns the board.

import { Button } from "@pidash/kit";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/document.js";
import { FORUM_URL } from "../../../shared/shell/externalLinks.js";

export const SPACE_CRASH_HEADING = "Something went wrong on this board";
export const SPACE_SUPPORT_HREF = "mailto:support@airepublic.com";

export function reloadSpaceCrash(): void {
  globalThis.window.location.reload();
}

export function SpaceCrashScreen(): React.ReactElement {
  useDocumentHead({ title: "Pi Dash — Board error", robots: "noindex, nofollow" });
  return (
    <main
      data-testid="space-crash"
      className="flex min-h-screen flex-col items-center justify-center gap-(--space-5) bg-(--bg) px-(--space-8) text-center text-(--text)"
    >
      <h1 className="text-h2">{SPACE_CRASH_HEADING}</h1>
      <p className="text-body max-w-lg text-(--text-muted)">
        This board could not be displayed. Refresh to try again; if it keeps happening, contact support or ask in the
        community forum.
      </p>
      <Button variant="primary" onClick={reloadSpaceCrash}>
        Refresh
      </Button>
      <p className="text-body flex gap-(--space-4) text-(--text-muted)">
        <a href={SPACE_SUPPORT_HREF} className="underline">
          Email support
        </a>
        <a href={FORUM_URL} target="_blank" rel="noreferrer" className="underline">
          Community forum
        </a>
      </p>
    </main>
  );
}
