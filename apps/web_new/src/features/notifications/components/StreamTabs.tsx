// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inbox stream tabs (written from parity/specs/notifications.md §2,
// NTF-002). Exactly two tabs; each badge mirrors its server total and
// hides at zero. Tab state lives in the inbox view, never in the address.
import { Badge } from "@pidash/kit";
import * as React from "react";

import { NOTIFICATION_TABS, type NotificationTab } from "../lib/stream.js";

const TAB_LABELS: Record<NotificationTab, string> = { all: "All", mentions: "Mentions" };

export interface StreamTabsProps {
  active: NotificationTab;
  totalUnread: number;
  mentionUnread: number;
  onSelect: (tab: NotificationTab) => void;
}

export function StreamTabs({ active, totalUnread, mentionUnread, onSelect }: StreamTabsProps): React.ReactElement {
  const counts: Record<NotificationTab, number> = { all: totalUnread, mentions: mentionUnread };
  return (
    <div
      role="tablist"
      aria-label="Inbox streams"
      className="flex shrink-0 gap-(--space-1) border-b border-(--border) px-(--space-3)"
    >
      {NOTIFICATION_TABS.map((tab) => {
        const selected = tab === active;
        const count = counts[tab];
        return (
          <button
            key={tab}
            type="button"
            role="tab"
            aria-selected={selected}
            data-testid={`inbox-tab-${tab}`}
            onClick={() => onSelect(tab)}
            className={`text-body flex items-center gap-(--space-2) px-(--space-2) py-(--space-3) ${
              selected
                ? "border-b-2 border-(--accent) text-(--text)"
                : "border-b-2 border-transparent text-(--text-muted) hover:text-(--text)"
            }`}
          >
            <span>{TAB_LABELS[tab]}</span>
            {count > 0 ? <Badge>{count}</Badge> : null}
          </button>
        );
      })}
    </div>
  );
}
