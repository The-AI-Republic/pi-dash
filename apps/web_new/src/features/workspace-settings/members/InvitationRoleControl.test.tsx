// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-row role control (WSET-010, spec §11): the trigger reads the
// invitation's role, the options stop at the actor's own role, and a pick
// reports the invitation id with the chosen number.
import type { WorkspaceInvitation } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { InvitationRoleControl } from "./InvitationRoleControl.js";

function invitation(role: number): WorkspaceInvitation {
  return {
    id: "inv-1",
    workspace: { id: "ws", name: "Acme", slug: "acme", logo_url: null },
    email: "ada@example.com",
    accepted: false,
    token: "tok",
    message: null,
    responded_at: null,
    role,
    invite_link: "",
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

function setup(role = 15, actorRole: number | null = 20, disabled = false) {
  const onChange = vi.fn();
  const view = render(
    <InvitationRoleControl
      invitation={invitation(role)}
      actorRole={actorRole}
      disabled={disabled}
      onChange={onChange}
    />
  );
  return { onChange, ...view };
}

describe("InvitationRoleControl", () => {
  it("reads the invitation's stored role", () => {
    setup(5);
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toHaveTextContent("Guest");
  });

  it("offers the actor's assignable roles and reports a pick by invitation id", async () => {
    const { onChange } = setup(15, 20);
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual([
      "Guest",
      "Member",
      "Admin",
    ]);
    await userEvent.click(screen.getByRole("option", { name: "Guest" }));
    expect(onChange).toHaveBeenCalledWith("inv-1", 5);
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toHaveTextContent("Member");
  });

  it("ignores a pick of the current role", async () => {
    const { onChange } = setup(15, 20);
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    await userEvent.click(screen.getByRole("option", { name: "Member" }));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("stops at a lower actor's own role", async () => {
    setup(5, 5);
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["Guest"]);
  });

  it("shows the stored role after a successful or refused refresh", async () => {
    const { rerender } = setup(15, 20);
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    await userEvent.click(screen.getByRole("option", { name: "Admin" }));
    rerender(
      <InvitationRoleControl invitation={invitation(20)} actorRole={20} disabled={false} onChange={() => undefined} />
    );
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toHaveTextContent("Admin");
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    await userEvent.click(screen.getByRole("option", { name: "Guest" }));
    rerender(
      <InvitationRoleControl invitation={invitation(20)} actorRole={20} disabled={false} onChange={() => undefined} />
    );
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toHaveTextContent("Admin");
  });

  it("locks while a write is in flight", () => {
    setup(15, 20, true);
    expect(screen.getByRole("button", { name: "Role for ada@example.com" })).toBeDisabled();
  });
});
