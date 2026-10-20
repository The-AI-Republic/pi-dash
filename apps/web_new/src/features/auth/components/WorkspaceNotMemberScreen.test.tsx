// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: its actions now resolve to router links
// since the auth prefixes migrated, and this suite owns the hrefs, not
// TanStack's navigation.
vi.mock("@tanstack/react-router", () => ({
  Link: (props: { to?: string; children?: React.ReactNode; ref?: React.Ref<HTMLAnchorElement> }) => (
    <a data-router-link="" data-to={props.to} href={props.to} ref={props.ref}>
      {props.children}
    </a>
  ),
}));

import { WorkspaceNotMemberScreen } from "./WorkspaceNotMemberScreen.js";

describe("WorkspaceNotMemberScreen", () => {
  it("points at invitations and creation with the first action focused", () => {
    render(<WorkspaceNotMemberScreen />);
    expect(screen.getByRole("heading", { name: "Not Authorized!" })).toBeVisible();
    const invites = screen.getByRole("link", { name: "View pending invitations" });
    expect(invites).toHaveFocus();
    expect(invites).toHaveAttribute("href", "/invitations");
    expect(screen.getByRole("link", { name: "Create a workspace" })).toHaveAttribute("href", "/create-workspace");
  });
});
