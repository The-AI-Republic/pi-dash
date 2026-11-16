// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row menu (WSET-013, spec §13): admins get Remove on another active row,
// everyone gets Leave on their own, and suspended rows and non-admin
// others' rows render no trigger at all.
import type { WorkspaceRosterMember } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { MemberActions } from "./MemberActions.js";

function member(userId: string, active = true): WorkspaceRosterMember {
  return {
    id: `m-${userId}`,
    member: {
      id: userId,
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: null,
      display_name: "Ada Lovelace",
      is_bot: false,
      email: `${userId}@example.com`,
      last_login_medium: "email",
    },
    role: 15,
    is_active: active,
    created_at: "2026-01-01T00:00:00Z",
  };
}

function setup(overrides: Partial<React.ComponentProps<typeof MemberActions>> = {}) {
  const onRemove = vi.fn();
  const onLeave = vi.fn();
  const view = render(
    <MemberActions
      row={member("other")}
      actorUserId="self"
      isAdmin
      onRemove={onRemove}
      onLeave={onLeave}
      {...overrides}
    />
  );
  return { onRemove, onLeave, ...view };
}

describe("MemberActions", () => {
  it("offers an admin Remove on somebody else's row", async () => {
    const { onRemove, onLeave } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Actions for other@example.com" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Remove" }));
    expect(onRemove).toHaveBeenCalledWith(expect.objectContaining({ id: "m-other" }));
    expect(onLeave).not.toHaveBeenCalled();
  });

  it("offers Leave on the actor's own row, even to an admin", async () => {
    const { onRemove, onLeave } = setup({ row: member("self") });
    await userEvent.click(screen.getByRole("button", { name: "Actions for self@example.com" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Leave" }));
    expect(onLeave).toHaveBeenCalledTimes(1);
    expect(onRemove).not.toHaveBeenCalled();
  });

  it("offers a non-admin only their own leave entry", async () => {
    setup({ isAdmin: false, row: member("self") });
    await userEvent.click(screen.getByRole("button", { name: "Actions for self@example.com" }));
    expect(await screen.findByRole("menuitem", { name: "Leave" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Remove" })).not.toBeInTheDocument();
  });

  it("renders nothing for a non-admin looking at somebody else", () => {
    setup({ isAdmin: false });
    expect(screen.queryByRole("button", { name: /Actions for/ })).not.toBeInTheDocument();
  });

  it("renders nothing on a suspended row", () => {
    setup({ row: member("gone", false) });
    expect(screen.queryByRole("button", { name: /Actions for/ })).not.toBeInTheDocument();
  });
});
