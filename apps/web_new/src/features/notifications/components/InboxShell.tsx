// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Two-pane inbox shell (written from parity/specs/notifications.md §1,
// NTF-001). The list pane keeps a fixed share of the width on desktop
// while the detail pane takes the visibly larger remainder; on narrow
// viewports the list takes the full width until a selection collapses it
// (selection and collapse belong to the detail slice). The detail pane
// content arrives with that slice; this shell owns the regions.
import * as React from "react";

export interface InboxShellProps {
  list: React.ReactNode;
  detail: React.ReactNode;
  selected?: boolean;
}

export function InboxShell({ list, detail, selected = false }: InboxShellProps): React.ReactElement {
  return (
    <div data-testid="inbox-shell" className="flex h-full min-h-0">
      <section
        aria-label="Notifications"
        data-testid="inbox-list-pane"
        className={`${selected ? "hidden md:flex" : "flex"} w-full shrink-0 flex-col overflow-y-auto border-r border-(--border) md:w-80 lg:w-96`}
      >
        {list}
      </section>
      <section
        aria-label="Details"
        data-testid="inbox-detail-pane"
        className={`${selected ? "block" : "hidden md:block"} min-w-0 flex-1`}
      >
        {detail}
      </section>
    </div>
  );
}
