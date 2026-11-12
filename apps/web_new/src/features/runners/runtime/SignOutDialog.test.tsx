// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

const canSignOut = vi.hoisted(() => ({ value: false }));

vi.mock("./service.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./service.js")>();
  return { ...original, canSignOutComputer: () => canSignOut.value };
});

import { confirmRuntimeSignOut, registerSignOutPrompt } from "./signOutPrompt.js";
import { RuntimeSignOutDialog } from "./SignOutDialog.js";

afterEach(() => {
  registerSignOutPrompt(null);
  canSignOut.value = false;
});

describe("confirmRuntimeSignOut", () => {
  it("keeps history when no dialog is mounted", async () => {
    await expect(confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false, signOutComputer: false });
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
    await expect(pending).resolves.toEqual({ deleteChatHistory: true, signOutComputer: false });
  });

  it("confirms with history kept by default", async () => {
    const user = userEvent.setup();
    render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Sign out" }));
    await expect(pending).resolves.toEqual({ deleteChatHistory: false, signOutComputer: false });
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
    await expect(confirmRuntimeSignOut()).resolves.toEqual({ deleteChatHistory: false, signOutComputer: false });
  });

  it("settles an open request on unmount", async () => {
    const { unmount } = render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    unmount();
    await expect(pending).resolves.toBeNull();
  });

  it("hides the computer sign-out switch when the daemon is not the account's", async () => {
    const user = userEvent.setup();
    canSignOut.value = false;
    render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    expect(screen.queryByRole("switch", { name: "Also sign out Pi Dash on this computer" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Sign out" }));
    await expect(pending).resolves.toEqual({ deleteChatHistory: false, signOutComputer: false });
  });

  it("offers the computer sign-out only when ticked", async () => {
    const user = userEvent.setup();
    canSignOut.value = true;
    render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    expect(screen.getByRole("switch", { name: "Also sign out Pi Dash on this computer" })).toBeVisible();
    // Untick: no computer logout.
    await user.click(screen.getByRole("button", { name: "Sign out" }));
    await expect(pending).resolves.toEqual({ deleteChatHistory: false, signOutComputer: false });
  });

  it("signs out the computer when its switch is on", async () => {
    const user = userEvent.setup();
    canSignOut.value = true;
    render(<RuntimeSignOutDialog />);
    const pending = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    await user.click(screen.getByRole("switch", { name: "Also sign out Pi Dash on this computer" }));
    await user.click(screen.getByRole("button", { name: "Sign out" }));
    await expect(pending).resolves.toEqual({ deleteChatHistory: false, signOutComputer: true });
  });

  it("starts the computer switch off on every open", async () => {
    const user = userEvent.setup();
    canSignOut.value = true;
    render(<RuntimeSignOutDialog />);
    const first = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    await user.click(screen.getByRole("switch", { name: "Also sign out Pi Dash on this computer" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(first).resolves.toBeNull();
    const second = confirmRuntimeSignOut();
    expect(await screen.findByText("Sign out?")).toBeVisible();
    expect(screen.getByRole("switch", { name: "Also sign out Pi Dash on this computer" })).toHaveAttribute(
      "aria-checked",
      "false"
    );
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(second).resolves.toBeNull();
  });
});
