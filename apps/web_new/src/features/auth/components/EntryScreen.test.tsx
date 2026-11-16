// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The entry page frame: what the root entry (and a cloud /login before its
// card lands) hands the sign-in card.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("./SignInCard.js", () => ({
  SignInCard: (props: {
    next?: string;
    unsafeNext?: string;
    initialEmail?: string;
    initialErrorCode?: string | null;
    invitationId?: string;
    invitationSlug?: string;
  }) => (
    <div
      data-testid="sign-in-card"
      data-next={props.next ?? ""}
      data-unsafe-next={props.unsafeNext ?? ""}
      data-email={props.initialEmail ?? ""}
      data-error={props.initialErrorCode ?? ""}
      data-invitation={props.invitationId ?? ""}
      data-slug={props.invitationSlug ?? ""}
    />
  ),
}));

import { EntryScreen } from "./EntryScreen.js";

describe("EntryScreen", () => {
  it("renders the sign-in card with the return path for signed-out visitors", () => {
    render(<EntryScreen next="/acme/" />);
    expect(screen.getByTestId("sign-in-card")).toHaveAttribute("data-next", "/acme/");
  });

  it("passes the card params through to the sign-in card", () => {
    render(
      <EntryScreen
        next="/acme"
        email="ada@example.com"
        errorCode="5065"
        invitationId="invite-1"
        invitationSlug="acme"
      />
    );
    const card = screen.getByTestId("sign-in-card");
    expect(card).toHaveAttribute("data-next", "/acme");
    expect(card).toHaveAttribute("data-unsafe-next", "");
    expect(card).toHaveAttribute("data-email", "ada@example.com");
    expect(card).toHaveAttribute("data-error", "5065");
    expect(card).toHaveAttribute("data-invitation", "invite-1");
    expect(card).toHaveAttribute("data-slug", "acme");
  });

  it("passes an unsafe return path through to the sign-in card", () => {
    render(<EntryScreen unsafeNext="https://evil.test/phish" />);
    const card = screen.getByTestId("sign-in-card");
    expect(card).toHaveAttribute("data-next", "");
    expect(card).toHaveAttribute("data-unsafe-next", "https://evil.test/phish");
  });

  it("defaults the initial email to an empty field", () => {
    render(<EntryScreen />);
    expect(screen.getByTestId("sign-in-card")).toHaveAttribute("data-email", "");
  });
});
