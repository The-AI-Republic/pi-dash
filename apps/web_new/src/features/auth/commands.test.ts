// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { runSignOutAction, SIGN_OUT_COMMAND_ID, signOutCommand } from "./commands.js";

describe("signOutCommand", () => {
  it("builds the stable sign-out descriptor every surface shares", () => {
    const onSignOut = vi.fn();
    const command = signOutCommand({ userLabel: "ada@example.com", onSignOut });
    expect(command.id).toBe(SIGN_OUT_COMMAND_ID);
    expect(SIGN_OUT_COMMAND_ID).toBe("shell.sign-out");
    expect(command.title).toBe("Sign out");
    expect(command.hint).toBe("ada@example.com");
    expect(command.section).toBe("Account");
    command.run();
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });
});

describe("runSignOutAction", () => {
  it("hands the landing target to the caller for navigation", async () => {
    const onLanded = vi.fn();
    await runSignOutAction(async () => "/", onLanded);
    expect(onLanded).toHaveBeenCalledWith("/");
  });

  it("stays put when the mutation rejects (the hook already toasted)", async () => {
    const onLanded = vi.fn();
    await runSignOutAction(async () => {
      throw new Error("boom");
    }, onLanded);
    expect(onLanded).not.toHaveBeenCalled();
  });

  it("stays put when the user cancels the desktop confirmation", async () => {
    const onLanded = vi.fn();
    await runSignOutAction(async () => null, onLanded);
    expect(onLanded).not.toHaveBeenCalled();
  });
});
