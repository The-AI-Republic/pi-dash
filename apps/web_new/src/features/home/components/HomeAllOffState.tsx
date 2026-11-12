// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// All-widgets-off illustration (SHELL-020; written from parity/specs/shell.md
// §2). When every surfaced widget is toggled off, this themed guidance
// replaces the stack until the first widget returns. Colors come from the
// theme tokens, so the art follows light and dark mode.

import { LayoutGrid } from "lucide-react";
import * as React from "react";

export function HomeAllOffState(): React.ReactElement {
  return (
    <div
      data-testid="home-all-off"
      className="flex flex-col items-center gap-(--space-2) px-(--space-4) py-(--space-12) text-center"
    >
      <span
        aria-hidden="true"
        className="grid size-12 place-items-center rounded-(--radius-full) bg-(--subtle) text-(--text-muted)"
      >
        <LayoutGrid size={24} strokeWidth={1.5} aria-hidden="true" />
      </span>
      <p className="text-emphasis text-(--text)">Your dashboard is empty</p>
      <p className="text-body text-(--text-muted)">
        Turn on a widget to fill this space — your links and recent activity will appear here.
      </p>
    </div>
  );
}
