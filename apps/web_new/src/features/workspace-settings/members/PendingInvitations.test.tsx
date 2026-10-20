// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending invitations group (WSET-010, spec §11): expanded by default with
// a count of search-matching rows, collapsing hides the rows, every row
// carries its role control and a menu with Copy link (only with a link)
// and Remove.
import type { WorkspaceInvitation } from "@pidash/api-client";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PendingInvitations } from "./PendingInvitations.js";

function invitation(
  email: string,
  created: string,
  link: string | null = "https://app.test/invite/tok"
): WorkspaceInvitation {
  return {
    id: `inv-${email}`,
    workspace: { id: "ws", name: "Acme", slug: "acme", logo_url: null },
    email,
    accepted: false,
    token: "tok",
    message: null,
    responded_at: null,
    role: 15,
    invite_link: link ?? "",
    created_by: null,
    updated_by: null,
    created_at: created,
    updated_at: created,
  };
}

const INVITATIONS = [
  invitation("ada@example.com", "2026-03-01T00:00:00Z"),
  invitation("grace@example.com", "2026-01-01T00:00:00Z", null),
];

function setup(overrides: Partial<React.ComponentProps<typeof PendingInvitations>> = {}) {
  const onRoleChange = vi.fn();
  const onCopyLink = vi.fn();
  const onRemove = vi.fn();
  const view = render(
    <PendingInvitations
      invitations={INVITATIONS}
      search=""
      actorRole={20}
      pending={false}
      onRoleChange={onRoleChange}
      onCopyLink={onCopyLink}
      onRemove={onRemove}
      {...overrides}
    />
  );
  return { onRoleChange, onCopyLink, onRemove, ...view };
}

function pendingRows(): HTMLElement[] {
  return screen.queryAllByTestId("pending-invitation-row");
}

describe("PendingInvitations", () => {
  it("renders nothing while the search leaves no invitation", () => {
    setup({ search: "nobody" });
    expect(screen.queryByTestId("pending-invitations")).not.toBeInTheDocument();
  });

  it("renders nothing for an empty list", () => {
    setup({ invitations: [] });
    expect(screen.queryByTestId("pending-invitations")).not.toBeInTheDocument();
  });

  it("opens expanded, oldest first, with the count of matching rows", () => {
    setup();
    const toggle = screen.getByRole("button", { name: /Pending invites/ });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("pending-invitations-count")).toHaveTextContent("2");
    expect(pendingRows().map((row) => row.getAttribute("data-email"))).toEqual([
      "grace@example.com",
      "ada@example.com",
    ]);
    expect(within(pendingRows()[1]!).getByText("Pending")).toBeInTheDocument();
  });

  it("narrows to the search and counts what remains", async () => {
    const { rerender } = setup();
    rerender(
      <PendingInvitations
        invitations={INVITATIONS}
        search="ADA"
        actorRole={20}
        pending={false}
        onRoleChange={() => undefined}
        onCopyLink={() => undefined}
        onRemove={() => undefined}
      />
    );
    expect(pendingRows().map((row) => row.getAttribute("data-email"))).toEqual(["ada@example.com"]);
    expect(screen.getByTestId("pending-invitations-count")).toHaveTextContent("1");
  });

  it("collapses to the header and reopens with the rows", async () => {
    setup();
    await userEvent.click(screen.getByRole("button", { name: /Pending invites/ }));
    expect(screen.getByRole("button", { name: /Pending invites/ })).toHaveAttribute("aria-expanded", "false");
    expect(pendingRows()).toHaveLength(0);
    await userEvent.click(screen.getByRole("button", { name: /Pending invites/ }));
    expect(pendingRows()).toHaveLength(2);
  });

  it("shows each invitation's role and offers the actor's assignable roles", async () => {
    setup();
    const trigger = within(pendingRows()[0]!).getByRole("button", { name: "Role for grace@example.com" });
    expect(trigger).toHaveTextContent("Member");
    await userEvent.click(trigger);
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual([
      "Guest",
      "Member",
      "Admin",
    ]);
  });

  it("offers a lower actor only lower roles", async () => {
    setup({ actorRole: 15 });
    await userEvent.click(within(pendingRows()[0]!).getByRole("button", { name: "Role for grace@example.com" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual([
      "Guest",
      "Member",
    ]);
  });

  it("reports the chosen role with the invitation id", async () => {
    const { onRoleChange } = setup();
    await userEvent.click(within(pendingRows()[1]!).getByRole("button", { name: "Role for ada@example.com" }));
    await userEvent.click(await screen.findByRole("option", { name: "Admin" }));
    expect(onRoleChange).toHaveBeenCalledWith("inv-ada@example.com", 20);
  });

  it("offers Copy link only for a row carrying one, and asks the caller to copy it", async () => {
    const { onCopyLink, onRemove } = setup();
    await userEvent.click(within(pendingRows()[0]!).getByRole("button", { name: "Actions for grace@example.com" }));
    expect(await screen.findByRole("menuitem", { name: "Remove" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Copy link" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("menuitem", { name: "Remove" }));
    expect(onRemove).toHaveBeenCalledWith(expect.objectContaining({ email: "grace@example.com" }));

    await userEvent.click(within(pendingRows()[1]!).getByRole("button", { name: "Actions for ada@example.com" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Copy link" }));
    expect(onCopyLink).toHaveBeenCalledWith(expect.objectContaining({ email: "ada@example.com" }));
  });

  it("asks to remove the invitation from the same menu", async () => {
    const { onRemove } = setup();
    await userEvent.click(within(pendingRows()[1]!).getByRole("button", { name: "Actions for ada@example.com" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Remove" }));
    expect(onRemove).toHaveBeenCalledWith(expect.objectContaining({ id: "inv-ada@example.com" }));
  });

  it("locks the role control while a write is in flight", () => {
    setup({ pending: true });
    expect(within(pendingRows()[0]!).getByRole("button", { name: "Role for grace@example.com" })).toBeDisabled();
  });
});
