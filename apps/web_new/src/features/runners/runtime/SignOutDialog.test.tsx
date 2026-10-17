// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { confirmRuntimeSignOut, registerSignOutPrompt, RuntimeSignOutDialog } from "./SignOutDialog.js";

afterEach(() => {
  registerSignOutPrompt(null);
});

describe("confirmRuntimeSignOut", () => {
  it("keeps history when no dialog is mounted", async () => {
    await expect(confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false });
  });
});

describe("RuntimeSignOutDialog", () => {
  it("confirms with history deletion when the switch is on", async () => {
    const user = userEvent.setup();
    render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    await user.click(screen.getByRole("switch", { name: "Also delete chat history" }));
    await user.click(screen.getByRole("button", { name: "Sign out" }));
    await expect(pending).resolves.toEqual({ deleteChatHistory: true });
  });

  it("confirms with history kept by default", async () => {
    const user = userEvent.setup();
    render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Sign out" }));
    await expect(pending).resolves.toEqual({ deleteChatHistory: false });
  });

  it("cancels through the cancel control", async () => {
    const user = userEvent.setup();
    render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(pending).resolves.toBeNull();
  });

  it("cancels on escape", async () => {
    const user = userEvent.setup();
    render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    await user.keyboard("{Escape}");
    await expect(pending).resolves.toBeNull();
  });

  it("starts the switch off on every open", async () => {
    const user = userEvent.setup();
    render(<RuntimeSignOutDialog />);
    const first = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    await user.click(screen.getByRole("switch", { name: "Also delete chat history" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(first).resolves.toBeNull();
    const second = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    expect(screen.getByRole("switch", { name: "Also delete chat history" })).toHaveAttribute("aria-checked", "false");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(second).resolves.toBeNull();
  });

  it("replaces a pending request instead of leaving it waiting", async () => {
    const user = userEvent.setup();
    render(<RuntimeSignOutDialog />);
    const first = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    const second = confirmRuntimeSignOut();
    await expect(first).resolves.toBeNull();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(second).resolves.toBeNull();
  });

  it("unregisters on unmount", async () => {
    const { unmount } = render(<RuntimeSignOutDialog />);
    unmount();
    await expect(confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false });
  });

  it("settles an open request on unmount", async () => {
    const { unmount } = render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    unmount();
    await expect(pending).resolves.toBeNull();
  });
});
