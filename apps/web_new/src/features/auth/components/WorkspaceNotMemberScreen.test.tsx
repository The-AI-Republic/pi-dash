// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { WorkspaceNotMemberScreen } from "./WorkspaceNotMemberScreen.js";

describe("WorkspaceNotMemberScreen", () => {
  it("points at invitations and creation with the first action focused", () => {
    render(<WorkspaceNotMemberScreen />);
    expect(screen.getByRole("heading", { name: "Not a member of this workspace" })).toBeVisible();
    const invites = screen.getByRole("link", { name: "View pending invitations" });
    expect(invites).toHaveFocus();
    expect(invites).toHaveAttribute("href", "/invitations");
    expect(screen.getByRole("link", { name: "Create a workspace" })).toHaveAttribute("href", "/create-workspace");
  });
});
