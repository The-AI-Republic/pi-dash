// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the auth card: its internals belong to the auth area.
// What this suite owns is the search schema, the entry branch decision,
// plus prop passing.
vi.mock("../features/auth/index.js", () => ({
  SignInCard: (props: {
    next?: string;
    initialEmail?: string;
    initialErrorCode?: string | null;
    invitationId?: string;
    invitationSlug?: string;
  }) => (
    <div
      data-testid="sign-in-card"
      data-next={props.next ?? ""}
      data-email={props.initialEmail ?? ""}
      data-error={props.initialErrorCode ?? ""}
      data-invitation={props.invitationId ?? ""}
      data-slug={props.invitationSlug ?? ""}
    />
  ),
}));

import { EntryScreen, parseEntrySearch } from "./index.js";

describe("parseEntrySearch", () => {
  it("keeps every known param", () => {
    expect(
      parseEntrySearch({
        next: "/acme",
        email: "ada@example.com",
        error_code: "5065",
        invitation_id: "invite-1",
        slug: "acme",
      })
    ).toEqual({
      next: "/acme",
      email: "ada@example.com",
      error_code: "5065",
      invitation_id: "invite-1",
      slug: "acme",
    });
  });

  it("drops invalid params instead of throwing", () => {
    expect(parseEntrySearch({})).toEqual({});
    expect(
      parseEntrySearch({ next: "https://evil.test/", email: "", error_code: 42, invitation_id: "", slug: null })
    ).toEqual({});
  });

  it("keeps a partial search", () => {
    expect(parseEntrySearch({ next: "/acme", error_code: "5900" })).toEqual({ next: "/acme", error_code: "5900" });
  });
});

describe("EntryScreen", () => {
  it("renders the sign-in card with the return path for signed-out visitors", () => {
    render(<EntryScreen signedIn={false} next="/acme/" />);
    expect(screen.getByTestId("sign-in-card")).toHaveAttribute("data-next", "/acme/");
    expect(screen.queryByRole("heading", { name: "Welcome to Pi Dash" })).toBeNull();
  });

  it("passes the card params through to the sign-in card", () => {
    render(
      <EntryScreen
        signedIn={false}
        next="/acme"
        email="ada@example.com"
        errorCode="5065"
        invitationId="invite-1"
        invitationSlug="acme"
      />
    );
    const card = screen.getByTestId("sign-in-card");
    expect(card).toHaveAttribute("data-next", "/acme");
    expect(card).toHaveAttribute("data-email", "ada@example.com");
    expect(card).toHaveAttribute("data-error", "5065");
    expect(card).toHaveAttribute("data-invitation", "invite-1");
    expect(card).toHaveAttribute("data-slug", "acme");
  });

  it("renders the workspace-less notice for signed-in visitors", () => {
    render(<EntryScreen signedIn={true} />);
    expect(screen.getByRole("heading", { name: "Welcome to Pi Dash" })).not.toBeNull();
    expect(screen.queryByTestId("sign-in-card")).toBeNull();
  });
});
