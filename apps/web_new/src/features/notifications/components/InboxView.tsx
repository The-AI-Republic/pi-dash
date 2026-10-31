// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Connected inbox view (written from parity/specs/notifications.md §1-§9,
// §11; NTF-001-023, NTF-026-030). Owns ephemeral tab, origin filter, mode
// and selection state, the two entry reads (paginated list per stream plus
// unread totals), refresh and bulk mark-all-read, the card writes (read,
// archive, snooze) and the document title; renders the shell, the stream
// controls, the applied chips, the tabs, the resolved cards, the detail
// host and the loading/empty/next-page states. Each tab, filter and mode
// combination is its own stream read from the first page, so any change
// lands on a fresh query instead of a continuation. No state is reflected
// into the route, so a remount always returns unselected. Nothing re-reads
// on its own: arrivals show up only after refresh, a tab/filter change or
// a remount (NTF-028). No role reads here: every workspace role renders
// this same view (NTF-026), and one implementation serves every build
// (NTF-030).
import * as React from "react";

import { useWorkspace } from "../../../core/session/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import {
  useMarkAllRead,
  useSnoozeNotification,
  useToggleNotificationArchive,
  useToggleNotificationRead,
} from "../api/mutations.js";
import { useMarkNotificationRead } from "../api/read.js";
import { useNotificationPages, useUnreadNotifications } from "../api/queries.js";
import { resolveNotificationCards, type NotificationCardData } from "../lib/card.js";
import { presetResumeInstant, type SnoozePreset } from "../lib/snooze.js";
import {
  DEFAULT_STREAM_MODES,
  isHiddenByStreamMode,
  markAllReadScope,
  notificationStreamQuery,
  removeOrigin,
  toggleMode,
  toggleOrigin,
  type NotificationOrigin,
  type NotificationTab,
  type OriginFilter,
  type StreamMode,
  type StreamModes,
} from "../lib/stream.js";
import { useBackgroundFetchGauge } from "../lib/useBackgroundFetch.js";
import { useStreamReset } from "../lib/useStreamReset.js";
import { CustomSnoozeDialog } from "./CustomSnoozeDialog.js";
import { InboxShell } from "./InboxShell.js";
import { NotificationCard, type NotificationCardActions } from "./NotificationCard.js";
import { NotificationEmptyState, NotificationListSkeleton, NotificationNextPage } from "./ListStates.js";
import { NotificationDetailHost } from "./NotificationDetailHost.js";
import { OriginChips } from "./OriginChips.js";
import { StreamControls } from "./StreamControls.js";
import { StreamTabs } from "./StreamTabs.js";

export function InboxView({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement {
  const { workspace } = useWorkspace(workspaceSlug);
  const [tab, setTab] = React.useState<NotificationTab>("all");
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const [origins, setOrigins] = React.useState<OriginFilter>([]);
  const [modes, setModes] = React.useState<StreamModes>(DEFAULT_STREAM_MODES);
  const [customFor, setCustomFor] = React.useState<string | null>(null);
  const [pendingId, setPendingId] = React.useState<string | null>(null);
  const attemptedReads = React.useRef(new Set<string>());
  const unread = useUnreadNotifications(workspaceSlug);
  const query = React.useMemo(() => notificationStreamQuery(tab, origins, modes), [tab, origins, modes]);
  const list = useNotificationPages(workspaceSlug, query);
  const markRead = useMarkNotificationRead(workspaceSlug);
  const toggleRead = useToggleNotificationRead(workspaceSlug);
  const toggleArchive = useToggleNotificationArchive(workspaceSlug);
  const snooze = useSnoozeNotification(workspaceSlug);
  const bulkRead = useMarkAllRead();
  const gauge = useBackgroundFetchGauge();
  const markingReadRef = React.useRef(false);
  const refreshing = gauge.count > 0;
  useDocumentHead({
    title: workspace === undefined ? `${workspaceSlug} · Inbox` : `${workspace.name} · Inbox`,
    robots: "noindex, nofollow",
  });

  // The list and the unread totals are one snapshot of the inbox: every
  // permitted trigger re-reads both, so a card and its badge never
  // disagree (spec §2, §7, §11). NTF-028 leaves no other trigger.
  const reloadStream = React.useCallback(async () => {
    await Promise.all([list.refetch(), unread.refetch()]);
  }, [list, unread]);

  const refresh = React.useCallback(() => {
    // Single flight: a press while a re-read runs is dropped, and so is
    // one while the first fetch has not landed. A cached stream keeps
    // showing its cards during the re-read, so the list's pending flag
    // stays false; the gauge and isFetching together cover both.
    if (refreshing || list.isFetching || unread.isFetching) return;
    const finish = gauge.begin();
    void reloadStream().finally(finish);
  }, [reloadStream, refreshing, gauge, list.isFetching, unread.isFetching]);

  const markAllRead = React.useCallback(() => {
    // A ref lock, not just isPending: a second press can land in the same
    // paint as the first, before the pending state re-renders.
    if (markingReadRef.current) return;
    markingReadRef.current = true;
    bulkRead.mutate(
      { workspaceSlug, scope: markAllReadScope(origins, modes), tab },
      { onSettled: () => (markingReadRef.current = false) }
    );
  }, [bulkRead, origins, modes, tab, workspaceSlug]);

  const selectTab = React.useCallback((next: NotificationTab) => {
    setTab(next);
    setSelectedId(null);
  }, []);
  const changeOrigins = React.useCallback((next: OriginFilter) => setOrigins(next), []);
  const changeMode = React.useCallback((mode: StreamMode) => setModes((current) => toggleMode(current, mode)), []);

  const streamSignature = [tab, origins.join(","), modes.unreadOnly, modes.archived, modes.snoozed].join("|");
  useStreamReset(streamSignature, reloadStream);

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

  const nowMs = Date.now();
  const rows = (list.data?.pages.flatMap((page) => page.results) ?? []).filter(
    (row) => !isHiddenByStreamMode(row, modes, nowMs)
  );
  const cards = resolveNotificationCards(rows);
  const selection = cards.find((card) => card.id === selectedId) ?? null;
  const customCard = cards.find((card) => card.id === customFor) ?? null;
  const selectCard = (card: NotificationCardData): void => {
    setSelectedId(card.id);
    if (!card.unread || attemptedReads.current.has(card.id)) return;
    attemptedReads.current.add(card.id);
    void markRead.mutateAsync({ id: card.id, mentioned: card.mentioned }).catch(() => {
      attemptedReads.current.delete(card.id);
    });
  };

  return (
    <>
      <InboxShell
        selected={selection !== null}
        list={
          <>
            <StreamControls
              origins={origins}
              modes={modes}
              refreshing={refreshing}
              markingRead={bulkRead.isPending}
              onRefresh={refresh}
              onMarkAllRead={markAllRead}
              onToggleOrigin={(origin) => changeOrigins(toggleOrigin(origins, origin))}
              onToggleMode={changeMode}
            />
            <OriginChips
              origins={origins}
              onRemove={(origin: NotificationOrigin) => changeOrigins(removeOrigin(origins, origin))}
              onClearAll={() => changeOrigins([])}
            />
            <StreamTabs
              active={tab}
              totalUnread={unread.data?.total_unread_notifications_count ?? 0}
              mentionUnread={unread.data?.mention_unread_notifications_count ?? 0}
              onSelect={selectTab}
            />
            {list.isPending ? (
              <NotificationListSkeleton />
            ) : cards.length === 0 ? (
              <NotificationEmptyState tab={tab} />
            ) : (
              <>
                <ul aria-label="Notification cards" className="flex flex-col">
                  {cards.map((card) => (
                    <li key={card.id}>
                      <NotificationCard
                        card={card}
                        selected={card.id === selectedId}
                        onSelect={() => selectCard(card)}
                        actions={actionsFor(card)}
                      />
                    </li>
                  ))}
                </ul>
                {list.hasNextPage ? (
                  <NotificationNextPage
                    loading={list.isFetchingNextPage}
                    onLoad={() => {
                      void list.fetchNextPage();
                    }}
                  />
                ) : null}
              </>
            )}
          </>
        }
        detail={
          <div data-testid="inbox-detail-host" className="h-full">
            <NotificationDetailHost
              workspaceSlug={workspaceSlug}
              selection={selection}
              onClose={() => setSelectedId(null)}
            />
          </div>
        }
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
