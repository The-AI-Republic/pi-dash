// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { NotificationCardData } from "../lib/card.js";
import { SNOOZE_PRESETS } from "../lib/snooze.js";
import { NotificationCard, type NotificationCardActions } from "./NotificationCard.js";

function card(overrides: Partial<NotificationCardData> = {}): NotificationCardData {
  return {
    id: "523e4567-e89b-12d3-a456-426614174004",
    issueId: "123e4567-e89b-12d3-a456-426614174000",
    projectId: "223e4567-e89b-12d3-a456-426614174001",
    actorName: "Ada Lovelace",
    actorAvatarUrl: "https://cdn.test/ada.png",
    summary: "Ada Lovelace commented: looks good",
    reference: "ENG-12",
    title: "Ship it",
    createdAt: new Date(Date.now() - 30_000).toISOString(),
    unread: true,
    mentioned: false,
    triage: false,
    archivedAt: null,
    snoozedTill: null,
    streamFlags: { is_inbox_issue: true, is_intake_issue: false, is_mentioned_notification: false },
    ...overrides,
  };
}

describe("NotificationCard", () => {
  it("renders the actor, summary, reference, title and age", () => {
    render(<NotificationCard card={card()} />);
    expect(screen.getByTestId("inbox-card-summary")).toHaveTextContent("Ada Lovelace commented: looks good");
    expect(screen.getByTestId("inbox-card-reference")).toHaveTextContent("ENG-12");
    expect(screen.getByTestId("inbox-card-title")).toHaveTextContent("Ship it");
    expect(screen.getByTestId("inbox-card-actor")).toHaveTextContent("Ada Lovelace");
    expect(screen.getByTestId("inbox-card-age")).toHaveTextContent("just now");
  });

  it("marks an unread card with the dot and tint", () => {
    render(<NotificationCard card={card()} />);
    const article = screen.getByTestId("inbox-card");
    expect(article).toHaveAttribute("data-unread", "true");
    expect(article).toHaveAttribute("aria-label", "Ship it (unread)");
    expect(screen.getByTestId("inbox-card-unread-dot")).toBeInTheDocument();
  });

  it("clears the dot and tint once read", () => {
    render(<NotificationCard card={card({ unread: false })} />);
    const article = screen.getByTestId("inbox-card");
    expect(article).toHaveAttribute("data-unread", "false");
    expect(article).toHaveAttribute("aria-label", "Ship it (read)");
    expect(screen.queryByTestId("inbox-card-unread-dot")).not.toBeInTheDocument();
  });

  it("falls back to initials without an avatar", () => {
    render(<NotificationCard card={card({ actorAvatarUrl: null })} />);
    expect(screen.getByRole("img", { name: "Ada Lovelace" })).toHaveTextContent("AL");
  });

  it("shows the avatar image when one is set", () => {
    const { container } = render(<NotificationCard card={card()} />);
    expect(container.querySelector('img[src="https://cdn.test/ada.png"]')).toBeInTheDocument();
  });

  it("selects through its native button and reports the selected state", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<NotificationCard card={card()} selected onSelect={onSelect} />);
    const wrapper = screen.getByTestId("inbox-card");
    expect(wrapper).toHaveAttribute("data-selected", "true");
    const button = screen.getByTestId("inbox-card-select");
    expect(button).toHaveAttribute("aria-pressed", "true");
    await user.click(button);
    expect(onSelect).toHaveBeenCalledOnce();
  });

  it("keeps the selection button and the action row as sibling controls", () => {
    render(<NotificationCard card={card()} actions={cardActions()} />);
    const row = screen.getByTestId("inbox-card-actions");
    expect(row).not.toContainElement(screen.getByTestId("inbox-card-select"));
    expect(screen.getByTestId("inbox-card-select")).toHaveAttribute("aria-pressed", "false");
  });

  it("keeps the action row hidden at rest and reveals it on this card's hover only", () => {
    const actions = cardActions();
    render(
      <>
        <NotificationCard card={card()} actions={actions} />
        <NotificationCard card={card({ id: "a23e4567-e89b-12d3-a456-426614174009" })} actions={cardActions()} />
      </>
    );
    const rows = screen.getAllByTestId("inbox-card-actions");
    expect(rows).toHaveLength(2);
    for (const row of rows) expect(row).toHaveClass("opacity-0");
    expect(screen.getAllByTestId("inbox-card-actions")[0]).toHaveClass("group-hover:opacity-100");
  });

  it("renders read, archive, then snooze in the fixed order", () => {
    render(<NotificationCard card={card()} actions={cardActions()} />);
    const row = screen.getByTestId("inbox-card-actions");
    const labels = within(row)
      .getAllByRole("button")
      .map((button) => button.getAttribute("aria-label"));
    expect(labels).toEqual(["Mark as read", "Archive", "Snooze notification"]);
  });

  it("labels the read and archive toggles by their next state", () => {
    render(
      <>
        <NotificationCard card={card({ unread: false })} actions={cardActions()} />
        <NotificationCard
          card={card({ id: "a23e4567-e89b-12d3-a456-426614174009", archivedAt: "2026-10-17T10:00:00Z" })}
          actions={cardActions()}
        />
      </>
    );
    expect(screen.getByRole("button", { name: "Mark as unread" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Unarchive" })).toBeInTheDocument();
  });

  it("runs the actions without bubbling the click to the card", async () => {
    const user = userEvent.setup();
    const actions = cardActions();
    const onCardPointer = vi.fn();
    render(
      <div onClick={onCardPointer}>
        <NotificationCard card={card()} actions={actions} />
      </div>
    );
    await user.click(screen.getByTestId("inbox-card-read-action"));
    await user.click(screen.getByTestId("inbox-card-archive-action"));
    expect(actions.onToggleRead).toHaveBeenCalledTimes(1);
    expect(actions.onToggleArchive).toHaveBeenCalledTimes(1);
    expect(onCardPointer).not.toHaveBeenCalled();
  });

  it("passes preset snoozes and custom picks through", async () => {
    const user = userEvent.setup();
    const actions = cardActions();
    render(<NotificationCard card={card()} actions={actions} />);
    await user.click(screen.getByTestId("snooze-trigger"));
    await user.click(await screen.findByTestId("snooze-option-week-1"));
    expect(actions.onPresetSnooze).toHaveBeenCalledWith(SNOOZE_PRESETS[3]);
  });

  it("keeps actions disabled while a write settles", () => {
    render(<NotificationCard card={card()} actions={cardActions({ busy: true })} />);
    expect(screen.getByTestId("inbox-card-read-action")).toBeDisabled();
    expect(screen.getByTestId("inbox-card-archive-action")).toBeDisabled();
    expect(screen.getByTestId("snooze-trigger")).toBeDisabled();
  });

  it("shows the scheduled resume moment instead of the age on a snoozed card", () => {
    render(
      <NotificationCard
        card={card({ snoozedTill: new Date(2026, 9, 12, 10, 30).toISOString(), createdAt: "2026-10-01T00:00:00Z" })}
        actions={cardActions()}
      />
    );
    const time = screen.getByTestId("inbox-card-age");
    expect(time).toHaveTextContent("Till Oct 12, 2026, 10:30 AM");
    expect(time).toHaveAttribute("dateTime", new Date(2026, 9, 12, 10, 30).toISOString());
  });

  it("keeps the relative age on an unsnoozed card", () => {
    render(<NotificationCard card={card({ createdAt: new Date(Date.now() - 30_000).toISOString() })} />);
    expect(screen.getByTestId("inbox-card-age")).toHaveTextContent("just now");
  });

  it("renders no action row when the card has no actions", () => {
    render(<NotificationCard card={card()} />);
    expect(screen.queryByTestId("inbox-card-actions")).not.toBeInTheDocument();
    expect(screen.queryByTestId("snooze-trigger")).not.toBeInTheDocument();
  });

  it("stops pointer-down as well as click on the action row", () => {
    const actions = cardActions();
    const onPointerDown = vi.fn();
    render(
      <div onPointerDown={onPointerDown}>
        <NotificationCard card={card()} actions={actions} />
      </div>
    );
    fireEvent.pointerDown(screen.getByTestId("inbox-card-actions"));
    fireEvent.keyDown(screen.getByTestId("inbox-card-actions"), { key: "Enter" });
    expect(onPointerDown).not.toHaveBeenCalled();
  });
});

function cardActions(overrides: Partial<NotificationCardActions> = {}): NotificationCardActions {
  return {
    onToggleRead: vi.fn(),
    onToggleArchive: vi.fn(),
    onPresetSnooze: vi.fn(),
    onCustomSnooze: vi.fn(),
    onRemoveSnooze: vi.fn(),
    busy: false,
    ...overrides,
  };
}
