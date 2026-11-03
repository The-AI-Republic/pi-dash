// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

// Stand-in for the single-invitation screen: its internals belong to the
// auth area. What this suite owns is the search schema plus prop passing.
vi.mock("../features/auth/components/SingleInvitation.js", () => ({
  SingleInvitation: (props: { invitationId?: string; slug?: string; token?: string }) => (
    <div
      data-testid="single-invitation"
      data-invitation={props.invitationId ?? ""}
      data-slug={props.slug ?? ""}
      data-token={props.token ?? ""}
    />
  ),
}));

import { parseWorkspaceInvitationSearch, Route, WorkspaceInvitationScreen } from "./workspace-invitations.js";

describe("parseWorkspaceInvitationSearch", () => {
  it("keeps every known param", () => {
    expect(parseWorkspaceInvitationSearch({ invitation_id: "invite-1", slug: "acme", token: "token-1" })).toEqual({
      invitation_id: "invite-1",
      slug: "acme",
      token: "token-1",
    });
  });

  it("drops invalid params instead of throwing", () => {
    expect(parseWorkspaceInvitationSearch({})).toEqual({});
    expect(parseWorkspaceInvitationSearch({ invitation_id: "", slug: 42, token: null })).toEqual({});
  });

  it("keeps a partial search", () => {
    expect(parseWorkspaceInvitationSearch({ slug: "acme" })).toEqual({ slug: "acme" });
    expect(parseWorkspaceInvitationSearch({ invitation_id: "invite-1", token: "token-1" })).toEqual({
      invitation_id: "invite-1",
      token: "token-1",
    });
  });
});

describe("WorkspaceInvitationScreen", () => {
  it("passes the link params through to the single-invitation screen", () => {
    render(<WorkspaceInvitationScreen search={{ invitation_id: "invite-1", slug: "acme", token: "token-1" }} />);
    const screen_ = screen.getByTestId("single-invitation");
    expect(screen_).toHaveAttribute("data-invitation", "invite-1");
    expect(screen_).toHaveAttribute("data-slug", "acme");
    expect(screen_).toHaveAttribute("data-token", "token-1");
  });

  it("passes an absent search through as empty", () => {
    render(<WorkspaceInvitationScreen search={{}} />);
    const screen_ = screen.getByTestId("single-invitation");
    expect(screen_).toHaveAttribute("data-invitation", "");
    expect(screen_).toHaveAttribute("data-slug", "");
    expect(screen_).toHaveAttribute("data-token", "");
  });
});

describe("workspace-invitation route component", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("reads the emailed-link params off the search", () => {
    vi.spyOn(Route, "useSearch").mockReturnValue({ invitation_id: "invite-1", slug: "acme", token: "token-1" });
    const Component = Route.options.component;
    if (!Component) throw new Error("workspace-invitation route has no component");
    render(<Component />);
    const screen_ = screen.getByTestId("single-invitation");
    expect(screen_).toHaveAttribute("data-invitation", "invite-1");
    expect(screen_).toHaveAttribute("data-slug", "acme");
    expect(screen_).toHaveAttribute("data-token", "token-1");
  });
});
