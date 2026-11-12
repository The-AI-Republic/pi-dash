// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Save-status indicator (parity/specs/issues.md §13, ISS-146). Renders
// the shared title-plus-description pipeline: a spinner with Saving
// while a write is in flight, then Saved fading out. The faded note
// stays mounted (transparent, ignored by assistive tech) so drivers
// and tests read a stable node.

import { Spinner } from "@pidash/kit";
import * as React from "react";

import type { SaveStatus } from "./savePipeline.js";

/** Delay before the Saved note fades out. */
export const SAVED_FADE_MS = 2000;

export function SaveIndicator({ status }: { status: SaveStatus }): React.ReactElement {
  const [faded, setFaded] = React.useState(false);
  React.useEffect(() => {
    if (status === "saving") {
      setFaded(false);
      return;
    }
    const timer = setTimeout(() => setFaded(true), SAVED_FADE_MS);
    return () => clearTimeout(timer);
  }, [status]);
  if (status === "saving") {
    return (
      <p role="status" className="text-meta flex items-center gap-(--space-2) text-(--text-muted)">
        <Spinner size="small" label="Saving" />
        <span aria-hidden="true">Saving…</span>
      </p>
    );
  }
  return (
    <p
      role="status"
      className={`text-meta text-(--text-muted) motion-safe:transition-opacity ${faded ? "opacity-0" : "opacity-100"}`}
    >
      Saved
    </p>
  );
}
