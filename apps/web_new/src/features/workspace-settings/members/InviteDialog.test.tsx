// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk invite dialog (WSET-009, spec §10): rows start at Member, append
// and drop, malformed addresses paint inline and block the send, a
// refused send keeps the input, and every row carries its own role.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { INVITE_DIALOG_TITLE, InviteDialog } from "./InviteDialog.js";

function setup(overrides: Partial<React.ComponentProps<typeof InviteDialog>> = {}) {
  const onSubmit = vi.fn();
  const onOpenChange = vi.fn();
  const view = render(
    <InviteDialog open actorRole={20} pending={false} onOpenChange={onOpenChange} onSubmit={onSubmit} {...overrides} />
  );
  return { onSubmit, onOpenChange, ...view };
}

function rows(): HTMLElement[] {
  return screen.queryAllByTestId(/^invite-row-/);
}

function field(index: number): HTMLInputElement {
  const input = document.querySelector<HTMLInputElement>(`input[name="emails.${index}.email"]`);
  if (input === null) throw new Error(`no email field ${index}`);
  return input;
}

/**
 * Type into a row the way a person does: click the field first. A dialog
 * that has just mounted may still be moving focus to its first control,
 * and typing into an unfocused node drops the keystrokes.
 */
async function typeRow(index: number, text: string): Promise<void> {
  await userEvent.click(field(index));
  await userEvent.type(field(index), text);
}

async function pickRowRole(label: string): Promise<void> {
  const trigger = screen.getAllByRole("button", { name: "Invitee role" })[0]!;
  await userEvent.click(trigger);
  await userEvent.click(await screen.findByRole("option", { name: label }));
}

describe("InviteDialog", () => {
  it("opens on one Member row with every assignable role behind the picker", async () => {
    setup();
    expect(screen.getByRole("heading", { name: INVITE_DIALOG_TITLE })).toBeInTheDocument();
    expect(rows()).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Invitee role" })).toHaveTextContent("Member");
    await userEvent.click(screen.getByRole("button", { name: "Invitee role" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual([
      "Guest",
      "Member",
      "Admin",
    ]);
  });

  it("limits a lower actor to the roles at or below their own", async () => {
    setup({ actorRole: 5 });
    expect(screen.getByRole("button", { name: "Invitee role" })).toHaveTextContent("Guest");
    await userEvent.click(screen.getByRole("button", { name: "Invitee role" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["Guest"]);
  });

  it("adds Member-defaulted rows and drops them, keeping one row alive", async () => {
    setup();
    await userEvent.click(screen.getByRole("button", { name: "Add more" }));
    expect(rows()).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "Invitee role" })).toHaveLength(2);
    await userEvent.click(screen.getAllByRole("button", { name: "Remove this invitee" })[1]!);
    expect(rows()).toHaveLength(1);
  });

  it("paints the invalid message under its row and sends nothing", async () => {
    const { onSubmit } = setup();
    await typeRow(0, "not-an-email");
    await userEvent.click(screen.getByRole("button", { name: "Send invitations" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Email is invalid");
    expect(onSubmit).not.toHaveBeenCalled();
    expect(field(0)).toHaveValue("not-an-email");
  });

  it("requires an address and clears the message once the row is edited", async () => {
    const { onSubmit } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Send invitations" }));
    expect(await screen.findByText("Email is required")).toBeInTheDocument();
    await typeRow(0, "ada@example.com");
    expect(screen.queryByText("Email is required")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Send invitations" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledWith([{ email: "ada@example.com", role: 15 }]));
  });

  it("sends each row with its own chosen role", async () => {
    const { onSubmit } = setup();
    await typeRow(0, "ada@example.com");
    await pickRowRole("Guest");
    await userEvent.click(screen.getByRole("button", { name: "Add more" }));
    await typeRow(1, "grace@example.com");
    const trigs = screen.getAllByRole("button", { name: "Invitee role" });
    await userEvent.click(trigs[1]!);
    await userEvent.click(await screen.findByRole("option", { name: "Admin" }));
    await userEvent.click(screen.getByRole("button", { name: "Send invitations" }));
    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith([
        { email: "ada@example.com", role: 5 },
        { email: "grace@example.com", role: 20 },
      ])
    );
  });

  it("reports a second malformed row alongside the first row's message", async () => {
    setup();
    await typeRow(0, "ada@example.com");
    await userEvent.click(screen.getByRole("button", { name: "Add more" }));
    await typeRow(1, "broken@");
    await userEvent.click(screen.getByRole("button", { name: "Send invitations" }));
    expect(await screen.findAllByRole("alert")).toHaveLength(1);
    expect(screen.getAllByRole("alert")[0]).toHaveTextContent("Email is invalid");
  });

  it("closes through Cancel", async () => {
    const { onOpenChange, onSubmit } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("closes on Escape like its sibling settings dialogs", async () => {
    const { onOpenChange } = setup();
    await userEvent.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledTimes(1);
    expect(onOpenChange.mock.calls[0]?.[0]).toBe(false);
  });

  it("locks every control while the send is in flight", () => {
    setup({ pending: true });
    expect(screen.getByRole("button", { name: "Send invitations" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add more" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Invitee role" })).toBeDisabled();
    expect(field(0)).not.toBeDisabled();
  });

  it("starts fresh on every open", async () => {
    const { rerender } = setup();
    await typeRow(0, "ada@example.com");
    rerender(
      <InviteDialog
        open={false}
        actorRole={20}
        pending={false}
        onOpenChange={() => undefined}
        onSubmit={() => undefined}
      />
    );
    rerender(
      <InviteDialog open actorRole={20} pending={false} onOpenChange={() => undefined} onSubmit={() => undefined} />
    );
    expect(field(0)).toHaveValue("");
  });
});
