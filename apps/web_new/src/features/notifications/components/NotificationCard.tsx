// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One inbox card (written from parity/specs/notifications.md §4, NTF-005,
// NTF-006). Four elements in server order: the actor, the change summary,
// the item reference with its title, and the relative age. Unread cards
// carry a marker dot and a shared tinted background; reading clears both.
// Selection and hover actions belong to the sibling slices — the card is
// not interactive here.
import { Avatar } from "@pidash/kit";
import * as React from "react";

import type { NotificationCardData } from "../lib/card.js";
import { formatNotificationAge } from "../lib/relativeAge.js";

export function NotificationCard({ card }: { card: NotificationCardData }): React.ReactElement {
  return (
    <article
      data-testid="inbox-card"
      data-unread={card.unread ? "true" : "false"}
      aria-label={`${card.title} (${card.unread ? "unread" : "read"})`}
      className={
        card.unread
          ? "flex gap-(--space-3) bg-(--accent-subtle) px-(--space-4) py-(--space-3)"
          : "flex gap-(--space-3) px-(--space-4) py-(--space-3)"
      }
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
          <time data-testid="inbox-card-age" dateTime={card.createdAt}>
            {formatNotificationAge(card.createdAt)}
          </time>
        </p>
      </div>
    </article>
  );
}
