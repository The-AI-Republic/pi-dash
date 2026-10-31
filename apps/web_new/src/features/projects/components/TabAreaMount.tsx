// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tab area mount (parity/specs/shell.md §7). The strip links every tab,
// but each tab's content belongs to its own area epic: cycles, modules,
// views, pages, intake and schedulers replace this mount with their
// screens when they land. Until then the address renders the area name
// inside the project layout instead of a dead end.

import * as React from "react";

export function TabAreaMount({ area }: { area: string }): React.ReactElement {
  return (
    <section aria-label={`${area} home`} className="flex min-h-0 flex-1 flex-col p-(--space-8)">
      <h1 className="text-h2 text-(--text)">{area}</h1>
    </section>
  );
}
