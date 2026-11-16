// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Roster table (WSET-008/011/042): column headers, sortable headers with
// their current direction marker, and the empty-safe body.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { WorkspaceRosterMember } from "@pidash/api-client";

import { MembersTable } from "./MembersTable.js";

function row(display: string, active = true): WorkspaceRosterMember {
  return {
    id: `m-${display}`,
    member: {
      id: `u-${display}`,
      first_name: "Ada",
      last_name: display,
      avatar: "",
      avatar_url: active ? null : null,
      display_name: display,
      is_bot: false,
      email: `${display.toLowerCase()}@example.com`,
      last_login_medium: "email",
    },
    role: 15,
    is_active: active,
    created_at: "2026-01-01T00:00:00Z",
  };
}

/** Row actions plus the actor props every table needs (WSET-012/013). */
const actions = {
  actorUserId: "u-someone",
  actorRole: 20,
  rolePending: false,
  onRoleChange: () => undefined,
  onRemove: () => undefined,
  onLeave: () => undefined,
};

describe("MembersTable", () => {
  it("renders the six column headers in order", () => {
    render(
      <MembersTable
        workspaceSlug="acme"
        rows={[row("Ada")]}
        order={null}
        onOrderChange={() => undefined}
        avatarUrl={() => null}
        {...actions}
      />
    );
    expect(screen.getAllByRole("columnheader").map((cell) => cell.textContent?.trim())).toEqual([
      "Full name",
      "Display name",
      "Email address",
      "Account type",
      "Authentication",
      "Joining date",
      "Row actions",
    ]);
  });

  it("marks the ascending direction on its sorted header", async () => {
    const onOrderChange = vi.fn();
    render(
      <MembersTable
        workspaceSlug="acme"
        rows={[row("Ada")]}
        order={{ column: "display_name", direction: "asc" }}
        onOrderChange={onOrderChange}
        avatarUrl={() => null}
        {...actions}
      />
    );
    const header = screen.getByRole("columnheader", { name: /Display name/ });
    expect(within(header).getByRole("button", { name: "Sort by Display name" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Sort by Display name" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Z → A" }));
    expect(onOrderChange).toHaveBeenCalledWith({ column: "display_name", direction: "desc" });
    await userEvent.click(screen.getByRole("button", { name: "Sort by Display name" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Clear sorting" }));
    expect(onOrderChange).toHaveBeenCalledWith(null);
  });

  it("marks the descending direction and offers both directions", async () => {
    const onOrderChange = vi.fn();
    render(
      <MembersTable
        workspaceSlug="acme"
        rows={[row("Ada")]}
        order={{ column: "role", direction: "desc" }}
        onOrderChange={onOrderChange}
        avatarUrl={() => null}
        {...actions}
      />
    );
    await userEvent.click(screen.getByRole("button", { name: "Sort by Account type" }));
    expect(await screen.findByRole("menuitem", { name: "Guest → Admin" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("menuitem", { name: "Guest → Admin" }));
    expect(onOrderChange).toHaveBeenCalledWith({ column: "role", direction: "asc" });
  });

  it("resolves each row's avatar through the caller's helper", () => {
    const avatarUrl = vi.fn(() => "https://api.test/media/photo.jpg");
    render(
      <MembersTable
        workspaceSlug="acme"
        rows={[row("Ada")]}
        order={null}
        onOrderChange={() => undefined}
        avatarUrl={avatarUrl}
        {...actions}
      />
    );
    expect(avatarUrl).toHaveBeenCalledWith(null);
    expect(screen.getByTestId("member-row").querySelector('[data-avatar-kind="image"]')).not.toBeNull();
  });

  it("renders an empty body without rows", () => {
    render(
      <MembersTable
        workspaceSlug="acme"
        rows={[]}
        order={null}
        onOrderChange={() => undefined}
        avatarUrl={() => null}
        {...actions}
      />
    );
    expect(screen.queryAllByTestId("member-row")).toEqual([]);
    expect(screen.getByRole("table", { name: "Members" })).toBeInTheDocument();
  });
});
