// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-members dialog (parity/specs/project-settings-members.md §"Add
// members"; PSET-022): one row to start, add/remove rows, the picker over
// workspace colleagues without a membership, roles following the picked
// colleague's workspace role, a per-row block on a blank pick, and one bulk
// submit. Also pins the dialog reset when it is reopened.
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ProjectMemberList, WorkspaceMemberList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { InviteMembersDialog, INVITE_DIALOG_TITLE } from "./InviteMembersDialog.js";
import { INVITE_MEMBER_REQUIRED } from "./writes.js";

const ADA = "123e4567-e89b-12d3-a456-42661417400a";
const GRACE = "123e4567-e89b-12d3-a456-42661417400b";
const LINUS = "123e4567-e89b-12d3-a456-42661417400c";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function entry(id: string, first: string, last: string, role: number, display = ""): WorkspaceMemberList[number] {
  return {
    id: `${id}-membership`,
    role,
    member: {
      id,
      first_name: first,
      last_name: last,
      avatar: "",
      avatar_url: null,
      display_name: display,
      is_bot: false,
    },
  };
}

const DIRECTORY: WorkspaceMemberList = [
  entry(ADA, "Ada", "Lovelace", 20),
  entry(GRACE, "Grace", "Hopper", 15),
  entry(LINUS, "Linus", "Torvalds", 5),
];

const MEMBERSHIPS: ProjectMemberList = [
  { id: "m-ada", role: 20, member: ADA, project: PROJECT, original_role: 20, created_at: "2025-01-01T00:00:00Z" },
];

function setup(
  overrides: { onSubmit?: (rows: { member_id: string; role: number }[]) => void; pending?: boolean } = {}
) {
  const onSubmit = overrides.onSubmit ?? vi.fn();
  function Harness(): React.ReactElement {
    const [open, setOpen] = React.useState(true);
    return (
      <InviteMembersDialog
        open={open}
        directory={DIRECTORY}
        memberships={MEMBERSHIPS}
        pending={overrides.pending ?? false}
        onOpenChange={setOpen}
        onSubmit={onSubmit}
      />
    );
  }
  return { onSubmit, Harness };
}

function renderDialog(props: Parameters<typeof setup>[0] = {}): ReturnType<typeof setup> {
  const harness = setup(props);
  render(<harness.Harness />);
  return harness;
}

afterEach(() => {
  cleanup();
});

describe("InviteMembersDialog", () => {
  it("opens with a single blank row and a disabled role picker", () => {
    renderDialog();
    expect(screen.getByRole("heading", { name: INVITE_DIALOG_TITLE })).toBeInTheDocument();
    expect(screen.getAllByTestId("invite-row")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Select co-worker" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Member role" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove row 1" })).toBeDisabled();
  });

  it("blocks a submit with no colleague and paints the row message", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Add members" }));
    expect(await screen.findByText(INVITE_MEMBER_REQUIRED)).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: INVITE_DIALOG_TITLE })).toBeInTheDocument();
  });

  it("offers colleagues not already in the project, with their display names", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole("button", { name: "Select co-worker" }));
    const options = await screen.findAllByRole("option");
    expect(options).toHaveLength(2);
    expect(options[0]).toHaveAccessibleName(/Grace Hopper/);
    expect(options[1]).toHaveAccessibleName(/Linus Torvalds/);
    expect(screen.queryByRole("option", { name: /Ada Lovelace/ })).not.toBeInTheDocument();
  });

  it("defaults the row role to the colleague's workspace role and caps its options", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole("button", { name: "Select co-worker" }));
    await user.click(await screen.findByRole("option", { name: /Grace Hopper/ }));
    expect(screen.getByRole("button", { name: "Member role" })).toHaveTextContent("Member");
    await user.click(screen.getByRole("button", { name: "Member role" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent)).toEqual([
      "Guest",
      "Member",
      "Admin",
    ]);
    await user.keyboard("{Escape}");

    // A workspace guest is capped at Guest only.
    await user.click(screen.getByRole("button", { name: "Remove row 1" }));
    expect(screen.getByRole("button", { name: "Remove row 1" })).toBeDisabled();
    expect(screen.getAllByTestId("invite-row")).toHaveLength(1);
  });

  it("adds a second row, picks a workspace guest there and caps it to Guest", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole("button", { name: "Add more" }));
    expect(screen.getAllByTestId("invite-row")).toHaveLength(2);
    const pickers = screen.getAllByRole("button", { name: "Select co-worker" });
    await user.click(pickers[1]!);
    await user.click(await screen.findByRole("option", { name: /Linus Torvalds/ }));
    const rolePickers = screen.getAllByRole("button", { name: "Member role" });
    expect(rolePickers[1]).toHaveTextContent("Guest");
    await user.click(rolePickers[1]!);
    expect((await screen.findAllByRole("option")).map((option) => option.textContent)).toEqual(["Guest"]);
    await user.keyboard("{Escape}");
    await user.click(screen.getAllByRole("button", { name: "Remove row 2" })[0]!);
    expect(screen.getAllByTestId("invite-row")).toHaveLength(1);
  });

  it("revalidates live once a submit has painted a row message", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole("button", { name: "Add members" }));
    expect(await screen.findByText(INVITE_MEMBER_REQUIRED)).toBeInTheDocument();
    // Adding a second blank row keeps the block, now for both rows.
    await user.click(screen.getByRole("button", { name: "Add more" }));
    expect(screen.getAllByText(INVITE_MEMBER_REQUIRED)).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(screen.queryByRole("heading", { name: INVITE_DIALOG_TITLE })).not.toBeInTheDocument();
  });

  it("submits every row in one bulk payload", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Select co-worker" }));
    await user.click(await screen.findByRole("option", { name: /Grace Hopper/ }));
    await user.click(screen.getByRole("button", { name: "Add more" }));
    const pickers = screen.getAllByRole("button", { name: "Select co-worker" });
    await user.click(pickers[1]!);
    await user.click(await screen.findByRole("option", { name: /Linus Torvalds/ }));
    await user.click(screen.getByRole("button", { name: "Add members" }));
    expect(onSubmit).toHaveBeenCalledWith([
      { member_id: GRACE, role: 15 },
      { member_id: LINUS, role: 5 },
    ]);
  });

  it("locks the controls while the bulk add flies", () => {
    renderDialog({ pending: true });
    expect(screen.getByRole("button", { name: "Select co-worker" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Member role" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add more" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
  });

  it("resets to one blank row after closing and reopening", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    function Harness(): React.ReactElement {
      const [open, setOpen] = React.useState(true);
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>
            reopen
          </button>
          <InviteMembersDialog
            open={open}
            directory={DIRECTORY}
            memberships={MEMBERSHIPS}
            pending={false}
            onOpenChange={setOpen}
            onSubmit={onSubmit}
          />
        </>
      );
    }
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Select co-worker" }));
    await user.click(await screen.findByRole("option", { name: /Grace Hopper/ }));
    await user.click(screen.getByRole("button", { name: "Add more" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: INVITE_DIALOG_TITLE })).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "reopen" }));
    expect(await screen.findByRole("heading", { name: INVITE_DIALOG_TITLE })).toBeInTheDocument();
    expect(screen.getAllByTestId("invite-row")).toHaveLength(1);
    const row = screen.getByTestId("invite-row");
    expect(within(row).getByRole("button", { name: "Select co-worker" })).toHaveTextContent("Select co-worker");
    expect(within(row).getByRole("button", { name: "Member role" })).toBeDisabled();
  });
});
