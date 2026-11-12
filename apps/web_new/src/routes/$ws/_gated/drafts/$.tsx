// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Drafts unknown-address boundary (DRAFT-024, from
// parity/specs/drafts.md §1.1/§2.1). There is no per-draft address, so
// /{ws}/drafts/{anything} lands here instead of a one-draft view. The
// surface keeps the shell's illustrated not-found treatment — artwork,
// heading, home action, crawler marker — and adds the one pinned phrase
// the parity contract asserts ("cannot be found"). No draft rows render.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { NOT_FOUND_ART, NOT_FOUND_ROBOTS, NOT_FOUND_TITLE, useDocumentHead } from "../../../../shared/shell/index.js";

/** Parity-pinned phrase (drafts spec §1.1): the unknown-draft body contains it. */
export const DRAFT_UNKNOWN_PHRASE = "cannot be found";

export const Route = createFileRoute("/$ws/_gated/drafts/$")({
  component: DraftUnknownAddress,
});

function DraftUnknownAddress(): React.ReactElement {
  useDocumentHead({ title: NOT_FOUND_TITLE, robots: NOT_FOUND_ROBOTS });
  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center">
      <img src={NOT_FOUND_ART} alt="An empty notice board with a magnifier" width={320} height={240} />
      <h1 className="text-h2 text-(--text)">That page does not exist</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        The draft you asked for {DRAFT_UNKNOWN_PHRASE}. The address may be mistyped, or the draft was moved or deleted.
        Head home and navigate from there.
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
