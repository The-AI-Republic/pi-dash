// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Connected inbox view (written from parity/specs/notifications.md §1,
// §2, §4, NTF-001-006). Owns the tab state, the two entry reads (list
// plus unread totals) and the document title; renders the shell, the
// stream tabs and the resolved cards. Detail content, loading and empty
// states, refresh, filters and card actions belong to the sibling slices.
// No role reads here: every workspace role renders this same view
// (NTF-026), and the one implementation serves every build (NTF-030).
import * as React from "react";

import { useWorkspace } from "../../../core/session/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { useNotificationsList, useUnreadNotifications } from "../api/queries.js";
import { resolveNotificationCards } from "../lib/card.js";
import { notificationTabQuery, type NotificationTab } from "../lib/stream.js";
import { InboxShell } from "./InboxShell.js";
import { NotificationCard } from "./NotificationCard.js";
import { StreamTabs } from "./StreamTabs.js";

export function InboxView({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement {
  const { workspace } = useWorkspace(workspaceSlug);
  const [tab, setTab] = React.useState<NotificationTab>("all");
  const unread = useUnreadNotifications(workspaceSlug);
  const list = useNotificationsList(workspaceSlug, notificationTabQuery(tab));
  useDocumentHead({
    title: workspace === undefined ? `${workspaceSlug} · Inbox` : `${workspace.name} · Inbox`,
    robots: "noindex, nofollow",
  });
  const cards = resolveNotificationCards(list.data?.results ?? []);
  return (
    <InboxShell
      list={
        <>
          <StreamTabs
            active={tab}
            totalUnread={unread.data?.total_unread_notifications_count ?? 0}
            mentionUnread={unread.data?.mention_unread_notifications_count ?? 0}
            onSelect={setTab}
          />
          <ul aria-label="Notification cards" className="flex flex-col">
            {cards.map((card) => (
              <li key={card.id}>
                <NotificationCard card={card} />
              </li>
            ))}
          </ul>
        </>
      }
      detail={<div data-testid="inbox-detail-host" className="h-full" />}
    />
  );
}
