// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the auth card: its internals belong to the auth area.
// What this suite owns is the search schema plus prop passing.
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

import { parseSignInSearch, safeNextPath, SignInScreen } from "./sign-in.js";

describe("safeNextPath", () => {
  it("keeps internal return paths", () => {
    expect(safeNextPath("/contract-acme")).toBe("/contract-acme");
    expect(safeNextPath("/a/projects/b/issues?layout=compact")).toBe("/a/projects/b/issues?layout=compact");
  });

  it("drops external, protocol-relative and empty values", () => {
    expect(safeNextPath("https://evil.test/")).toBeUndefined();
    expect(safeNextPath("//evil.test/")).toBeUndefined();
    expect(safeNextPath("")).toBeUndefined();
    expect(safeNextPath(undefined)).toBeUndefined();
    expect(safeNextPath(42)).toBeUndefined();
  });
});

describe("parseSignInSearch", () => {
  it("keeps every known param", () => {
    expect(
      parseSignInSearch({
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
    expect(parseSignInSearch({})).toEqual({});
    expect(
      parseSignInSearch({ next: "https://evil.test/", email: "", error_code: 42, invitation_id: "", slug: null })
    ).toEqual({});
  });

  it("keeps a partial search", () => {
    expect(parseSignInSearch({ email: "ada@example.com", slug: "acme" })).toEqual({
      email: "ada@example.com",
      slug: "acme",
    });
  });
});

describe("SignInScreen", () => {
  it("passes the search through to the card", () => {
    render(
      <SignInScreen
        search={{
          next: "/acme",
          email: "ada@example.com",
          error_code: "5065",
          invitation_id: "invite-1",
          slug: "acme",
        }}
      />
    );
    const card = screen.getByTestId("sign-in-card");
    expect(card).toHaveAttribute("data-next", "/acme");
    expect(card).toHaveAttribute("data-email", "ada@example.com");
    expect(card).toHaveAttribute("data-error", "5065");
    expect(card).toHaveAttribute("data-invitation", "invite-1");
    expect(card).toHaveAttribute("data-slug", "acme");
  });

  it("defaults a bare search to empty card props", () => {
    render(<SignInScreen search={{}} />);
    const card = screen.getByTestId("sign-in-card");
    expect(card).toHaveAttribute("data-next", "");
    expect(card).toHaveAttribute("data-email", "");
    expect(card).toHaveAttribute("data-error", "");
    expect(card).toHaveAttribute("data-invitation", "");
    expect(card).toHaveAttribute("data-slug", "");
  });
});
