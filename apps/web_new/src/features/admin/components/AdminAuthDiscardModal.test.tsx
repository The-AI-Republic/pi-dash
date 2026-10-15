// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Discard modal: the pinned title, body, and keep/leave controls with
// the landing href; keep-editing and any dialog dismiss preserve the
// draft through onKeep.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { AdminAuthDiscardModal } from "./AdminAuthDiscardModal.js";

describe("discard modal", () => {
  it("renders the pinned copy with the landing href when open", () => {
    render(<AdminAuthDiscardModal open backHref="/god-mode/authentication/" onKeep={() => {}} />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("You have unsaved changes")).toBeInTheDocument();
    expect(
      screen.getByText("Changes you made will be lost if you go back. Do you wish to go back?")
    ).toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-discard-keep")).toHaveTextContent("Keep editing");
    const leave = screen.getByTestId("admin-auth-discard-leave");
    expect(leave).toHaveTextContent("Go back");
    expect(leave).toHaveAttribute("href", "/god-mode/authentication/");
  });

  it("stays shut when closed", () => {
    render(<AdminAuthDiscardModal open={false} backHref="/god-mode/authentication/" onKeep={() => {}} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("keeps the edits on keep-editing", async () => {
    const user = userEvent.setup();
    const onKeep = vi.fn();
    render(<AdminAuthDiscardModal open backHref="/god-mode/authentication/" onKeep={onKeep} />);
    await user.click(screen.getByTestId("admin-auth-discard-keep"));
    expect(onKeep).toHaveBeenCalledTimes(1);
  });

  it("keeps the edits on dialog dismiss", async () => {
    const user = userEvent.setup();
    const onKeep = vi.fn();
    render(<AdminAuthDiscardModal open backHref="/god-mode/authentication/" onKeep={onKeep} />);
    await user.keyboard("{Escape}");
    expect(onKeep).toHaveBeenCalledTimes(1);
  });
});
