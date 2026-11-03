// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared remove/leave confirmation (WSET-010/013, spec §13): title and
// body name the target, Cancel and Escape close it untouched, the confirm
// button carries the verb, and a write in flight keeps it unescapable.
import type { WorkspaceInvitation, WorkspaceRosterMember } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { RemoveTargetDialog } from "./RemoveTargetDialog.js";

const INVITATION: WorkspaceInvitation = {
  id: "inv-1",
  workspace: { id: "ws", name: "Acme", slug: "acme", logo_url: null },
  email: "ada@example.com",
  accepted: false,
  token: "tok",
  message: null,
  responded_at: null,
  role: 15,
  invite_link: "/invite/tok",
  created_by: null,
  updated_by: null,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

const MEMBER: WorkspaceRosterMember = {
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
  role: 15,
  is_active: true,
  created_at: "2026-01-01T00:00:00Z",
};

function setup(overrides: Partial<React.ComponentProps<typeof RemoveTargetDialog>> = {}) {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  const view = render(
    <RemoveTargetDialog
      target={{ kind: "self" }}
      pending={false}
      onCancel={onCancel}
      onConfirm={onConfirm}
      {...overrides}
    />
  );
  return { onCancel, onConfirm, ...view };
}

describe("RemoveTargetDialog", () => {
  it("names another member and confirms removal", async () => {
    const { onConfirm } = setup({ target: { kind: "member", row: MEMBER } });
    expect(screen.getByRole("heading", { name: "Remove Ada Lovelace?" })).toBeInTheDocument();
    expect(screen.getByText(/no longer have access/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("asks the leaver before leaving", async () => {
    const { onConfirm } = setup({ target: { kind: "self" } });
    expect(screen.getByRole("heading", { name: "Leave workspace?" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Leave" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("names the invitation address", () => {
    setup({ target: { kind: "invitation", invitation: INVITATION } });
    expect(screen.getByRole("heading", { name: "Remove ada@example.com?" })).toBeInTheDocument();
    expect(screen.getByText(/This withdraws the pending invitation for ada@example\.com/)).toBeInTheDocument();
  });

  it("closes through Cancel without confirming", async () => {
    const { onCancel, onConfirm } = setup({ target: { kind: "self" } });
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("closes through Escape and the header control without confirming", async () => {
    const { onCancel, onConfirm, unmount } = setup({ target: { kind: "member", row: MEMBER } });
    expect(screen.getByRole("heading", { name: "Remove Ada Lovelace?" })).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
    unmount();

    const again = setup({ target: { kind: "member", row: MEMBER } });
    await userEvent.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(again.onCancel).toHaveBeenCalledTimes(1);
    expect(again.onConfirm).not.toHaveBeenCalled();
  });

  it("stays put while a write is in flight and reports nothing on a blocked confirm", () => {
    const { onCancel, onConfirm } = setup({ target: { kind: "self" }, pending: true });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Leave" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
