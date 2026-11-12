// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Illustrated not-found page (SHELL-108, from parity/specs/shell.md §1).
// Unknown addresses render this explanatory surface with a way home —
// never a blank screen, a stack trace or an empty shell — and crawlers
// are told to stay out. The home action is a plain anchor so it keeps
// working even when client routing is unhappy.

import * as React from "react";

import { useDocumentHead } from "./document.js";

export const NOT_FOUND_TITLE = "404 — Pi Dash";
export const NOT_FOUND_ROBOTS = "noindex, nofollow";
export const NOT_FOUND_ART = "/illustrations/not-found.svg";

export function NotFoundPage(): React.ReactElement {
  useDocumentHead({ title: NOT_FOUND_TITLE, robots: NOT_FOUND_ROBOTS });
  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center">
      <img src={NOT_FOUND_ART} alt="An empty notice board with a magnifier" width={320} height={240} />
      <h1 className="text-h2 text-(--text)">That page does not exist</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        The page was not found. The address may be mistyped, or the page was moved or deleted. Head home and navigate
        from there.
      </p>
      <a
        href="/"
        className="text-body rounded-(--radius-control) bg-(--accent) px-(--space-4) py-(--space-2) text-(--on-accent)"
      >
        Back to home
      </a>
    </main>
  );
}
