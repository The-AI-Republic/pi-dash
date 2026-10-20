// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row menu and confirmation (parity/specs/project-settings-members.md
// §"Remove and leave-self"; PSET-024/025): Remove for others, Leave on one's
// own row, cancel keeps the membership, confirm runs the write, and the
// pending state locks both buttons.
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { confirmationCopy, MemberRowMenu } from "./MemberRowMenu.js";

afterEach(() => {
  cleanup();
});

describe("confirmationCopy", () => {
  it("names the member and warns the access loss is permanent", () => {
    const copy = confirmationCopy("remove", "Grace Hopper");
    expect(copy.title).toBe("Remove Grace Hopper?");
    expect(copy.description).toContain("Grace Hopper");
    expect(copy.description).toContain("no longer have access");
    expect(copy.description).toContain("permanently");
    expect(copy.confirm).toBe("Remove");
  });

  it("asks to leave on one's own row", () => {
    const copy = confirmationCopy("leave", "Grace Hopper");
    expect(copy.title).toBe("Leave project?");
    expect(copy.description.toLowerCase()).toContain("lose access");
    expect(copy.confirm).toBe("Leave");
  });
});

describe("MemberRowMenu", () => {
  it("renders nothing when the row offers no action", () => {
    const { container } = render(
      <MemberRowMenu action="none" name="Grace Hopper" pending={false} onConfirm={vi.fn()} />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("offers Remove on another member's row", async () => {
    const user = userEvent.setup();
    render(<MemberRowMenu action="remove" name="Grace Hopper" pending={false} onConfirm={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Actions for Grace Hopper" }));
    expect(await screen.findByRole("menuitem", { name: "Remove" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Leave" })).not.toBeInTheDocument();
  });

  it("offers Leave on the caller's own row", async () => {
    const user = userEvent.setup();
    render(<MemberRowMenu action="leave" name="Grace Hopper" pending={false} onConfirm={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Actions for Grace Hopper" }));
    expect(await screen.findByRole("menuitem", { name: "Leave" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Remove" })).not.toBeInTheDocument();
  });

  it("confirms with the name in the copy and runs the write", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<MemberRowMenu action="remove" name="Grace Hopper" pending={false} onConfirm={onConfirm} />);
    await user.click(screen.getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Remove Grace Hopper?")).toBeInTheDocument();
    expect(within(dialog).getByText(/no longer have access/)).toBeInTheDocument();
    // The menu is gone once the confirmation opens.
    expect(screen.queryByRole("menuitem", { name: "Remove" })).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("cancels without running the write", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<MemberRowMenu action="leave" name="Grace Hopper" pending={false} onConfirm={onConfirm} />);
    await user.click(screen.getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Leave" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("dismisses the confirmation with Escape without running the write", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<MemberRowMenu action="remove" name="Grace Hopper" pending={false} onConfirm={onConfirm} />);
    await user.click(screen.getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("withdraws the confirmation once the write settles, even on failure", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    const { rerender } = render(
      <MemberRowMenu action="remove" name="Grace Hopper" pending={false} onConfirm={onConfirm} />
    );
    await user.click(screen.getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Remove" }));
    rerender(<MemberRowMenu action="remove" name="Grace Hopper" pending onConfirm={onConfirm} />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    // The refused write settles with the row still present.
    rerender(<MemberRowMenu action="remove" name="Grace Hopper" pending={false} onConfirm={onConfirm} />);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("keeps the confirmation open when nothing was submitted", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <MemberRowMenu action="remove" name="Grace Hopper" pending={false} onConfirm={vi.fn()} />
    );
    await user.click(screen.getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
    rerender(<MemberRowMenu action="remove" name="Grace Hopper" pending={false} onConfirm={vi.fn()} />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("locks the confirmation while the write flies", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <MemberRowMenu action="remove" name="Grace Hopper" pending={false} onConfirm={vi.fn()} />
    );
    await user.click(screen.getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
    rerender(<MemberRowMenu action="remove" name="Grace Hopper" pending onConfirm={vi.fn()} />);
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Remove" })).toBeDisabled();
    expect(within(dialog).queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
  });
});
