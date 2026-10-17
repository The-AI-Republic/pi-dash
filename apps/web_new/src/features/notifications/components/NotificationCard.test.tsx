// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import type { NotificationCardData } from "../lib/card.js";
import { NotificationCard } from "./NotificationCard.js";

function card(overrides: Partial<NotificationCardData> = {}): NotificationCardData {
  return {
    id: "523e4567-e89b-12d3-a456-426614174004",
    actorName: "Ada Lovelace",
    actorAvatarUrl: "https://cdn.test/ada.png",
    summary: "Ada Lovelace commented: looks good",
    reference: "ENG-12",
    title: "Ship it",
    createdAt: new Date(Date.now() - 30_000).toISOString(),
    unread: true,
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
});
