// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Connected inbox view (written from parity/specs/notifications.md §1,
// §2, §4, §9, NTF-001-006, NTF-018-023). Owns the tab state, the two
// entry reads (list plus unread totals), the card writes (read, archive,
// snooze) and the document title; renders the shell, the stream tabs and
// the resolved cards. Detail content, loading and empty states, refresh,
// filters and modes belong to the sibling slices. No role reads here:
// every workspace role renders this same view (NTF-026), and the one
// implementation serves every build (NTF-030).
import * as React from "react";

import { useWorkspace } from "../../../core/session/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { useSnoozeNotification, useToggleNotificationArchive, useToggleNotificationRead } from "../api/mutations.js";
import { useNotificationsList, useUnreadNotifications } from "../api/queries.js";
import { resolveNotificationCards, type NotificationCardData } from "../lib/card.js";
import { presetResumeInstant, type SnoozePreset } from "../lib/snooze.js";
import { notificationTabQuery, type NotificationTab } from "../lib/stream.js";
import { CustomSnoozeDialog } from "./CustomSnoozeDialog.js";
import { InboxShell } from "./InboxShell.js";
import { NotificationCard, type NotificationCardActions } from "./NotificationCard.js";
import { StreamTabs } from "./StreamTabs.js";

export function InboxView({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement {
  const { workspace } = useWorkspace(workspaceSlug);
  const [tab, setTab] = React.useState<NotificationTab>("all");
  const [customFor, setCustomFor] = React.useState<string | null>(null);
  const [pendingId, setPendingId] = React.useState<string | null>(null);
  const unread = useUnreadNotifications(workspaceSlug);
  const list = useNotificationsList(workspaceSlug, notificationTabQuery(tab));
  const toggleRead = useToggleNotificationRead(workspaceSlug);
  const toggleArchive = useToggleNotificationArchive(workspaceSlug);
  const snooze = useSnoozeNotification(workspaceSlug);
  useDocumentHead({
    title: workspace === undefined ? `${workspaceSlug} · Inbox` : `${workspace.name} · Inbox`,
    robots: "noindex, nofollow",
  });

  const track = React.useCallback(function track<T>(id: string, run: () => Promise<T>): Promise<T> {
    setPendingId(id);
    return run().finally(() => {
      setPendingId(null);
    });
  }, []);

  const actionsFor = React.useCallback(
    function actionsFor(card: NotificationCardData): NotificationCardActions {
      const flags = card.streamFlags;
      return {
        busy: pendingId === card.id,
        onToggleRead: () => {
          void track(card.id, () => toggleRead.mutateAsync({ id: card.id, unread: card.unread, flags })).catch(
            () => undefined
          );
        },
        onToggleArchive: () => {
          void track(card.id, () =>
            toggleArchive.mutateAsync({ id: card.id, archived: card.archivedAt === null, flags })
          ).catch(() => undefined);
        },
        onPresetSnooze: (preset: SnoozePreset) => {
          void track(card.id, () =>
            snooze.mutateAsync({
              id: card.id,
              snoozedTill: presetResumeInstant(preset, Date.now()).toISOString(),
              flags,
            })
          ).catch(() => undefined);
        },
        onCustomSnooze: () => {
          setCustomFor(card.id);
        },
        onRemoveSnooze: () => {
          void track(card.id, () => snooze.mutateAsync({ id: card.id, snoozedTill: null, flags })).catch(
            () => undefined
          );
        },
      };
    },
    [pendingId, snooze, toggleArchive, toggleRead, track]
  );

  const cards = resolveNotificationCards(list.data?.results ?? []);
  const customCard = cards.find((card) => card.id === customFor) ?? null;

  return (
    <>
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
                  <NotificationCard card={card} actions={actionsFor(card)} />
                </li>
              ))}
            </ul>
          </>
        }
        detail={<div data-testid="inbox-detail-host" className="h-full" />}
      />
      {customCard === null ? null : (
        <CustomSnoozeDialog
          open
          onOpenChange={(next) => {
            if (!next) setCustomFor(null);
          }}
          onSubmit={(resume) =>
            track(customCard.id, () =>
              snooze.mutateAsync({
                id: customCard.id,
                snoozedTill: resume.toISOString(),
                flags: customCard.streamFlags,
              })
            )
          }
        />
      )}
    </>
  );
}
