// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inbox continuation states (parity/specs/notifications.md §6,
// NTF-010-012): first-load skeletons, per-stream empty art and an explicit
// cursor continuation control.
import { Bell, MessageCircle } from "lucide-react";
import * as React from "react";

import { Button, EmptyState, Skeleton } from "@pidash/kit";
import type { NotificationTab } from "../lib/stream.js";

export function NotificationListSkeleton(): React.ReactElement {
  return (
    <div data-testid="notifications-skeleton" aria-label="Loading notifications" className="flex flex-col">
      {Array.from({ length: 5 }, (_, index) => (
        <div
          key={index}
          data-testid="notifications-skeleton-row"
          className="flex gap-(--space-3) px-(--space-4) py-(--space-3)"
        >
          <Skeleton width="24px" height="24px" round />
          <div className="flex flex-1 flex-col gap-(--space-2)">
            <Skeleton height="14px" />
            <Skeleton width="70%" height="12px" />
          </div>
        </div>
      ))}
    </div>
  );
}

export function NotificationEmptyState({ tab }: { tab: NotificationTab }): React.ReactElement {
  const mentions = tab === "mentions";
  return (
    <div data-testid="notifications-empty-state" data-empty-tab={tab}>
      <EmptyState
        variant="illustrated"
        art={mentions ? <MessageCircle size={64} strokeWidth={1.25} /> : <Bell size={64} strokeWidth={1.25} />}
        title={mentions ? "No mentions waiting" : "Nothing has arrived"}
        description={mentions ? "Mentions from your team will appear here." : "Updates to your work will collect here."}
        className="px-(--space-6)"
      />
    </div>
  );
}

export function NotificationNextPage({
  loading,
  onLoad,
}: {
  loading: boolean;
  onLoad: () => void;
}): React.ReactElement {
  return (
    <div className="flex justify-center px-(--space-4) py-(--space-4)">
      <Button size="small" variant="secondary" loading={loading} onClick={onLoad} data-testid="notifications-next-page">
        {loading ? "Loading older notifications" : "Show older notifications"}
      </Button>
    </div>
  );
}
