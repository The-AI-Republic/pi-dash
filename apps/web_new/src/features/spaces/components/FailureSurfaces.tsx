// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The two board-specific failure illustrations (SPC-003, SPC-004). They are
// distinct from the generic not-found page and never render board chrome.
// Heading levels are deliberate: the anchor case is a missing resource (h3),
// the board case is the page's primary failure (h1). Both keep their own
// title and crawler directive but carry the generic board description and
// social tags (SPC-007): the board head mounts beside them, and the newest
// head claim wins, so a tag-less claim here would strip the fallback tags.

import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/document.js";
import { buildSpaceBoardHead } from "../lib/boardHead.js";

export const SPACE_ANCHOR_NOT_FOUND_ART = "/illustrations/not-found.svg";
export const SPACE_BOARD_ERROR_ART = "/illustrations/instance-failure.svg";

const FALLBACK_HEAD_TAGS = buildSpaceBoardHead(undefined).tags;

export function SpaceAnchorNotFound(): React.ReactElement {
  useDocumentHead({ title: "Pi Dash — Board not found", robots: "noindex, nofollow", tags: FALLBACK_HEAD_TAGS });
  return (
    <main
      data-testid="space-anchor-not-found"
      className="flex min-h-screen flex-col items-center justify-center gap-(--space-5) bg-(--bg) px-(--space-8) text-center text-(--text)"
    >
      <img src={SPACE_ANCHOR_NOT_FOUND_ART} alt="An empty notice board" width={320} height={240} />
      <h3 className="text-h3">We couldn't find this board</h3>
      <p className="text-body max-w-md text-(--text-muted)">
        The board may have been withdrawn or the address may be mistyped.
      </p>
    </main>
  );
}

export function SpaceBoardError(): React.ReactElement {
  useDocumentHead({ title: "Pi Dash — Board error", robots: "noindex, nofollow", tags: FALLBACK_HEAD_TAGS });
  return (
    <section
      data-testid="space-board-error"
      className="flex min-h-[60vh] flex-col items-center justify-center gap-(--space-5) px-(--space-8) text-center text-(--text)"
    >
      <img src={SPACE_BOARD_ERROR_ART} alt="An unavailable server" width={320} height={240} />
      <h1 className="text-h2">We couldn't load this board</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        The board data did not arrive. Reload the page or come back in a moment.
      </p>
    </section>
  );
}
