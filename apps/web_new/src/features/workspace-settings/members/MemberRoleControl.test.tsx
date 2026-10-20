// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account-type control (WSET-012, spec §12): the trigger reads the row's
// role, the options are the actor's assignable roles, a pick reports the
// row and role, and the control adopts the settled role after a refresh.
import type { WorkspaceRosterMember } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { MemberRoleControl } from "./MemberRoleControl.js";

function member(role: number): WorkspaceRosterMember {
  return {
    id: "m1",
    member: {
      id: "u1",
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: null,
      display_name: "Ada Lovelace",
      is_bot: false,
      email: "ada@example.com",
      last_login_medium: "email",
    },
    role,
    is_active: true,
    created_at: "2026-01-01T00:00:00Z",
  };
}

function setup(role = 15, actorRole: number | null = 20, pending = false) {
  const onChange = vi.fn();
  const view = render(
    <MemberRoleControl row={member(role)} actorRole={actorRole} pending={pending} onChange={onChange} />
  );
  return { onChange, ...view };
}

describe("MemberRoleControl", () => {
  it("reads the row's stored role", () => {
    setup(5);
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toHaveTextContent("Guest");
  });

  it("offers the actor's assignable roles and reports a pick", async () => {
    const { onChange } = setup(15, 20);
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual([
      "Guest",
      "Member",
      "Admin",
    ]);
    await userEvent.click(screen.getByRole("option", { name: "Admin" }));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ id: "m1" }), 20);
    // The control reads the stored role until the refreshed row lands.
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toHaveTextContent("Member");
  });

  it("ignores a pick of the role the row already has", async () => {
    const { onChange } = setup(15, 20);
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    await userEvent.click(screen.getByRole("option", { name: "Member" }));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("limits a member actor to the lower roles", async () => {
    setup(5, 15);
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual([
      "Guest",
      "Member",
    ]);
  });

  it("shows the stored role after a successful refresh and keeps it after a refused one", async () => {
    const { rerender } = setup(15, 20);
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    await userEvent.click(screen.getByRole("option", { name: "Admin" }));
    rerender(<MemberRoleControl row={member(20)} actorRole={20} pending={false} onChange={() => undefined} />);
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toHaveTextContent("Admin");
    // A refused write leaves the old role in the refreshed row.
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    await userEvent.click(screen.getByRole("option", { name: "Guest" }));
    rerender(<MemberRoleControl row={member(20)} actorRole={20} pending={false} onChange={() => undefined} />);
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toHaveTextContent("Admin");
  });

  it("locks while a write is in flight", () => {
    setup(15, 20, true);
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toBeDisabled();
  });
});
