// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One inbox card (written from parity/specs/notifications.md §4, §9,
// NTF-005, NTF-006, NTF-022, NTF-023, NTF-007). Four elements in server
// order — the actor, the change summary, the item reference with its
// title, and the age — plus the unread marker. A snoozed card shows its
// scheduled resume moment where the age would be. The action row (read,
// archive, snooze) is revealed by hovering this card only, reserves its
// space at rest so nothing shifts, and every action stops its click from
// selecting the card. Selection is a native button covering the card body;
// the actions sit beside it rather than inside it.
import { Avatar } from "@pidash/kit";
import { Archive, ArchiveRestore, Mail, MailOpen } from "lucide-react";
import * as React from "react";

import type { NotificationCardData } from "../lib/card.js";
import { formatNotificationAge } from "../lib/relativeAge.js";
import { formatResumeLabel, type SnoozePreset } from "../lib/snooze.js";
import { SnoozeMenu } from "./SnoozeMenu.js";

export interface NotificationCardActions {
  /** Flip the card's read state; the write is optimistic. */
  onToggleRead: () => void;
  /** Archive or return the card. */
  onToggleArchive: () => void;
  /** Snooze to a preset delay. */
  onPresetSnooze: (preset: SnoozePreset) => void;
  /** Open the custom resume dialog for this card. */
  onCustomSnooze: () => void;
  /** Clear the card's snooze. */
  onRemoveSnooze: () => void;
  /** True while any write for this card is still settling. */
  busy: boolean;
}

export interface NotificationCardProps {
  card: NotificationCardData;
  /** Whether this card is the open detail selection. */
  selected?: boolean;
  /** Open this card's detail. */
  onSelect?: () => void;
  /** When present the card renders its hover action row. */
  actions?: NotificationCardActions | undefined;
}

function stopPointer(event: React.SyntheticEvent): void {
  event.stopPropagation();
}

const actionButtonClass =
  "inline-flex size-6 cursor-pointer items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text) disabled:cursor-not-allowed disabled:opacity-50";

function ActionRow({
  card,
  actions,
}: {
  card: NotificationCardData;
  actions: NotificationCardActions;
}): React.ReactElement {
  return (
    <div
      data-testid="inbox-card-actions"
      className="flex shrink-0 items-center gap-(--space-1) opacity-0 group-hover:opacity-100 focus-within:opacity-100"
      onClick={stopPointer}
      onPointerDown={stopPointer}
      onKeyDown={stopPointer}
    >
      <button
        type="button"
        aria-label={card.unread ? "Mark as read" : "Mark as unread"}
        data-testid="inbox-card-read-action"
        disabled={actions.busy}
        onClick={actions.onToggleRead}
        className={actionButtonClass}
      >
        {card.unread ? (
          <MailOpen size={14} strokeWidth={2} aria-hidden="true" />
        ) : (
          <Mail size={14} strokeWidth={2} aria-hidden="true" />
        )}
      </button>
      <button
        type="button"
        aria-label={card.archivedAt === null ? "Archive" : "Unarchive"}
        data-testid="inbox-card-archive-action"
        disabled={actions.busy}
        onClick={actions.onToggleArchive}
        className={actionButtonClass}
      >
        {card.archivedAt === null ? (
          <Archive size={14} strokeWidth={2} aria-hidden="true" />
        ) : (
          <ArchiveRestore size={14} strokeWidth={2} aria-hidden="true" />
        )}
      </button>
      <SnoozeMenu
        snoozed={card.snoozedTill !== null}
        busy={actions.busy}
        onPreset={actions.onPresetSnooze}
        onCustom={actions.onCustomSnooze}
        onRemove={actions.onRemoveSnooze}
      />
    </div>
  );
}

/** The card's age line: the resume moment when snoozed, age otherwise. */
function CardTime({ card }: { card: NotificationCardData }): React.ReactElement {
  if (card.snoozedTill !== null) {
    return (
      <time data-testid="inbox-card-age" dateTime={card.snoozedTill}>
        {formatResumeLabel(card.snoozedTill)}
      </time>
    );
  }
  return (
    <time data-testid="inbox-card-age" dateTime={card.createdAt}>
      {formatNotificationAge(card.createdAt)}
    </time>
  );
}

export function NotificationCard({
  card,
  selected = false,
  onSelect,
  actions,
}: NotificationCardProps): React.ReactElement {
  return (
    <div
      data-testid="inbox-card"
      data-unread={card.unread ? "true" : "false"}
      data-selected={selected ? "true" : "false"}
      aria-label={`${card.title} (${card.unread ? "unread" : "read"})`}
      className={
        selected || card.unread
          ? "group flex gap-(--space-3) bg-(--accent-subtle) px-(--space-4) py-(--space-3)"
          : "group flex gap-(--space-3) px-(--space-4) py-(--space-3)"
      }
    >
      <button
        type="button"
        data-testid="inbox-card-select"
        aria-pressed={selected}
        aria-label={`Open ${card.reference}`}
        onClick={onSelect}
        className="flex min-w-0 flex-1 cursor-pointer gap-(--space-3) text-left"
      >
        {card.unread ? (
          <span
            data-testid="inbox-card-unread-dot"
            aria-hidden="true"
            className="mt-1 size-2 shrink-0 rounded-full bg-(--accent)"
          />
        ) : null}
        <Avatar
          name={card.actorName}
          {...(card.actorAvatarUrl === null ? {} : { src: card.actorAvatarUrl })}
          size="small"
        />
        <div className="min-w-0 flex-1">
          <p data-testid="inbox-card-summary" className="text-body text-(--text)">
            {card.summary}
          </p>
          <p className="text-caption text-(--text-muted)">
            <span data-testid="inbox-card-reference" className="font-[family-name:var(--font-mono)]">
              {card.reference}
            </span>
            {" · "}
            <span data-testid="inbox-card-title">{card.title}</span>
          </p>
          <p className="text-caption text-(--text-muted)">
            <span data-testid="inbox-card-actor">{card.actorName}</span>
            {" · "}
            <CardTime card={card} />
          </p>
        </div>
      </button>
      {actions === undefined ? null : <ActionRow card={card} actions={actions} />}
    </div>
  );
}
